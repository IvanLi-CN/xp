use super::*;
use futures_util::StreamExt;
use http_body_util::BodyExt as _;
use reqwest::ResponseBuilderExt;
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub(crate) const REVERSE_MAX_IN_FLIGHT_PER_RENDEZVOUS: usize = 8;
pub(super) const REVERSE_HEALTH_RESERVED_SLOTS: usize = 1;
pub(super) type ReverseInFlight =
    Arc<tokio::sync::Mutex<std::collections::BTreeMap<String, Arc<Semaphore>>>>;

#[derive(Clone, Copy)]
pub(super) enum ReverseRequestClass {
    Control,
    Health,
}

pub(super) fn reverse_authority(route: &ReverseRelayRoute, peer: &MeshPeerTarget) -> String {
    crate::reverse_mesh::derive_reverse_authority(
        route.assignment.credential_epoch,
        &peer.node_id,
        &route.rendezvous.node_id,
        route.role,
        route.assignment.generation,
    )
}

#[derive(Debug, Clone)]
pub(super) struct LocalReverseRelay {
    pub(super) node_id: String,
    pub(super) base_url: String,
}

impl PeerCircuitBreakers {
    pub(super) async fn try_reverse_slot(
        &self,
        rendezvous_node_id: &str,
        class: ReverseRequestClass,
    ) -> Result<OwnedSemaphorePermit, MeshRequestError> {
        let mut limits = self.reverse_in_flight.lock().await;
        let semaphore = limits
            .entry(rendezvous_node_id.to_owned())
            .or_insert_with(|| Arc::new(Semaphore::new(REVERSE_MAX_IN_FLIGHT_PER_RENDEZVOUS)))
            .clone();
        if matches!(class, ReverseRequestClass::Control)
            && semaphore.available_permits() <= REVERSE_HEALTH_RESERVED_SLOTS
        {
            return Err(MeshRequestError::Reverse(format!(
                "reverse relay concurrency limit reached for rendezvous {rendezvous_node_id}"
            )));
        }
        semaphore.clone().try_acquire_owned().map_err(|_| {
            MeshRequestError::Reverse(format!(
                "reverse relay concurrency limit reached for rendezvous {rendezvous_node_id}"
            ))
        })
    }
}

pub(super) fn attach_reverse_slot(
    response: reqwest::Response,
    permit: OwnedSemaphorePermit,
) -> reqwest::Response {
    let response_url = response.url().clone();
    let response: axum::http::Response<reqwest::Body> = response.into();
    let (mut parts, body) = response.into_parts();
    let url_extensions = axum::http::Response::builder()
        .url(response_url)
        .body(())
        .expect("response URL extension builder")
        .into_parts()
        .0
        .extensions;
    let mut extensions = url_extensions;
    extensions.extend(std::mem::take(&mut parts.extensions));
    parts.extensions = extensions;
    let body = body.into_data_stream();
    let guarded_body = futures_util::stream::unfold((body, permit), |(mut body, permit)| async {
        let item = body.next().await?;
        Some((item, (body, permit)))
    });
    let response =
        axum::http::Response::from_parts(parts, reqwest::Body::wrap_stream(guarded_body));
    reqwest::Response::from(response)
}

impl MeshAwareHttpClient {
    pub(super) async fn record_reverse_sample(
        &self,
        peer: &MeshPeerTarget,
        started: Instant,
        request: &MeshRequest,
        route: &ReverseRelayRoute,
    ) {
        if let Some(telemetry) = &self.telemetry {
            let _ = telemetry
                .record_reverse_sample(crate::mesh_telemetry::ReverseRelayTelemetrySample {
                    peer_id: peer.node_id.clone(),
                    peer_name: peer.node_name.clone(),
                    rendezvous: route.rendezvous.node_id.clone(),
                    rendezvous_role: route.role.as_str().to_string(),
                    primary_rendezvous: route.assignment.primary_node_id.clone(),
                    standby_rendezvous: route.assignment.standby_node_id.clone(),
                    generation: route.assignment.generation,
                    sample: telemetry_sample(
                        TelemetryPath::Mesh,
                        true,
                        started.elapsed(),
                        true,
                        request.updates_active_path,
                        None,
                    ),
                })
                .await;
        }
    }

    /// Uses the local XP API as the portal when this process is the assigned Rendezvous.
    pub fn with_local_reverse_relay(
        mut self,
        node_id: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        self.local_reverse_relay = Some(LocalReverseRelay {
            node_id: node_id.into(),
            base_url: base_url.into(),
        });
        self
    }

    /// Probes every assigned rendezvous so a standby can serve immediately after failover.
    pub(crate) async fn send_peer_reverse_health_request(
        &self,
        peer: &MeshPeerTarget,
        request: MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
    ) -> Result<(), MeshRequestError> {
        if !self.reverse_enabled.load(Ordering::Acquire) {
            return Err(MeshRequestError::Reverse(
                "reverse relay is disabled until local Xray readiness recovers".to_string(),
            ));
        }
        if request.path_and_query != "/api/admin/_internal/mesh/health"
            || request.method != reqwest::Method::GET
            || !request.body.is_empty()
        {
            return Err(MeshRequestError::Reverse(
                "reverse health probe must be a bodyless GET".to_string(),
            ));
        }
        let route = self
            .reverse_routes
            .read()
            .await
            .get(&peer.node_id)
            .cloned()
            .ok_or_else(|| {
                MeshRequestError::Reverse("no reverse assignment is available".into())
            })?;
        let started = Instant::now();
        let mut first_error = None;
        for candidate in route.candidates() {
            let budget = route_budget(request.total_budget)
                .min(request.total_budget.saturating_sub(started.elapsed()));
            if budget.is_zero() {
                first_error.get_or_insert(MeshRequestError::OutcomeUnknown);
                break;
            }
            match self
                .send_reverse_relay(
                    peer,
                    &candidate,
                    &request,
                    cluster_ca_key_pem,
                    cluster_ca_cert_pem,
                    budget,
                    ReverseRequestClass::Health,
                )
                .await
            {
                Ok(_) => {}
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(())
    }

    /// Sends one health request through the specified assigned link. Target-side liveness uses
    /// this rather than the normal primary/standby fan-out so the signed response identifies one
    /// Xray underlay precisely.
    pub(crate) async fn send_peer_reverse_health_request_via(
        &self,
        peer: &MeshPeerTarget,
        route: &ReverseRelayRoute,
        request: MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
    ) -> Result<(), MeshRequestError> {
        if !self.reverse_enabled.load(Ordering::Acquire) {
            return Err(MeshRequestError::Reverse(
                "reverse relay is disabled until local Xray readiness recovers".to_string(),
            ));
        }
        if request.path_and_query != "/api/admin/_internal/mesh/health"
            || request.method != reqwest::Method::GET
            || !request.body.is_empty()
        {
            return Err(MeshRequestError::Reverse(
                "reverse health probe must be a bodyless GET".to_string(),
            ));
        }
        self.send_reverse_relay(
            peer,
            route,
            &request,
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
            route_budget(request.total_budget),
            ReverseRequestClass::Health,
        )
        .await
        .map(|_| ())
    }

    /// Sends only through the Raft-assigned Reverse route. This is used after a repository's
    /// equal direct paths have both failed, before the legacy encrypted dynamic relay is tried.
    pub(crate) async fn send_peer_reverse_request(
        &self,
        peer: &MeshPeerTarget,
        request: MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
    ) -> Result<reqwest::Response, MeshRequestError> {
        if !self.reverse_enabled.load(Ordering::Acquire) {
            return Err(MeshRequestError::Reverse(
                "reverse relay is disabled until local Xray readiness recovers".to_string(),
            ));
        }
        if request.path_and_query.contains("/mesh/reverse-relay") {
            return Err(MeshRequestError::Reverse(
                "recursive reverse relay is not allowed".to_string(),
            ));
        }
        let route = self
            .reverse_routes
            .read()
            .await
            .get(&peer.node_id)
            .cloned()
            .ok_or_else(|| {
                MeshRequestError::Reverse("no reverse assignment is available".into())
            })?;
        let started = Instant::now();
        let mut last_error = None;
        for candidate in route.candidates() {
            let budget = route_budget(request.total_budget)
                .min(request.total_budget.saturating_sub(started.elapsed()));
            if budget.is_zero() {
                break;
            }
            match self
                .send_reverse_relay(
                    peer,
                    &candidate,
                    &request,
                    cluster_ca_key_pem,
                    cluster_ca_cert_pem,
                    budget,
                    ReverseRequestClass::Control,
                )
                .await
            {
                Ok(response) => {
                    self.record_reverse_sample(peer, started, &request, &candidate)
                        .await;
                    return Ok(response);
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            MeshRequestError::Reverse("reverse assignment has no usable candidate".into())
        }))
    }
}

pub(super) async fn send_outer_request(
    client: &reqwest::Client,
    request: &MeshRequest,
    url: &str,
    headers: &axum::http::HeaderMap,
    budget: Duration,
    allow_ambiguous_fallback: bool,
) -> Result<reqwest::Response, MeshRequestError> {
    let mut builder = client
        .request(request.method.clone(), url)
        .body(request.body.clone());
    for (name, value) in headers {
        builder = builder.header(name, value);
    }
    match tokio::time::timeout(budget, builder.send()).await {
        Ok(result) => {
            result.map_err(|error| public_transport_error(error, allow_ambiguous_fallback))
        }
        Err(_) if allow_ambiguous_fallback => Err(MeshRequestError::Reverse(
            "reverse outer request timed out before response headers".to_string(),
        )),
        Err(_) => Err(MeshRequestError::OutcomeUnknown),
    }
}
