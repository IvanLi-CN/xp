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

#[allow(clippy::too_many_arguments)]
pub(super) fn verify_relay_ack(
    request: &MeshRequest,
    response: &reqwest::Response,
    verified: &internal_auth::VerifiedRequest,
    expected_node_id: &str,
    cluster_ca_key_pem: &str,
    cluster_ca_cert_pem: &str,
    header_name: &str,
    missing_message: &str,
) -> Result<(), MeshRequestError> {
    let ack = response
        .headers()
        .get(header_name)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            dispatch_error(request, MeshRequestError::Protocol(missing_message.into()))
        })?;
    if let Err(error) = internal_auth::verify_ack_v2(
        cluster_ca_key_pem,
        cluster_ca_cert_pem,
        verified,
        expected_node_id,
        response.status().as_u16(),
        ack,
    ) {
        return Err(dispatch_error(request, error.into()));
    }
    Ok(())
}

fn dispatch_error(request: &MeshRequest, error: MeshRequestError) -> MeshRequestError {
    if request.allow_ambiguous_fallback {
        error
    } else if matches!(
        error,
        MeshRequestError::ReverseTimeout | MeshRequestError::TransportTimeout
    ) {
        MeshRequestError::TransportTimeout
    } else {
        MeshRequestError::OutcomeUnknown
    }
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
    let guarded_body =
        futures_util::stream::unfold((body, Some(permit)), |(mut body, mut permit)| async move {
            match body.next().await {
                Some(Ok(item)) => Some((Ok(item), (body, permit))),
                Some(Err(error)) => {
                    drop(permit.take());
                    Some((Err(error), (body, permit)))
                }
                None => {
                    drop(permit.take());
                    None
                }
            }
        });
    let response =
        axum::http::Response::from_parts(parts, reqwest::Body::wrap_stream(guarded_body));
    reqwest::Response::from(response)
}

pub(super) fn attach_mesh_gate(
    response: reqwest::Response,
    gate_guard: tokio::sync::OwnedRwLockReadGuard<()>,
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
    let guarded_body = futures_util::stream::unfold(
        (body, Some(gate_guard)),
        |(mut body, mut gate_guard)| async move {
            match body.next().await {
                Some(Ok(item)) => Some((Ok(item), (body, gate_guard))),
                Some(Err(error)) => {
                    drop(gate_guard.take());
                    Some((Err(error), (body, gate_guard)))
                }
                None => {
                    drop(gate_guard.take());
                    None
                }
            }
        },
    );
    reqwest::Response::from(axum::http::Response::from_parts(
        parts,
        reqwest::Body::wrap_stream(guarded_body),
    ))
}

impl MeshAwareHttpClient {
    pub(super) async fn record_reverse_sample(
        &self,
        peer: &MeshPeerTarget,
        started: Instant,
        request: &MeshRequest,
        route: &ReverseRelayRoute,
        epoch: u64,
    ) {
        // Every successful reverse response carries the Mesh read guard in its body stream.
        // Do not reacquire the write-preferring lock here: a queued gate transition would
        // otherwise wait for this response while this task waits for another read lock.
        if !self.mesh_gate_matches(epoch) {
            return;
        }
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
    #[cfg(test)]
    pub(crate) async fn send_peer_reverse_health_request(
        &self,
        peer: &MeshPeerTarget,
        request: MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
    ) -> Result<(), MeshRequestError> {
        if !self.cluster_mesh_enabled.load(Ordering::Acquire) {
            return Err(MeshRequestError::Reverse(
                "reverse relay is disabled by the cluster Mesh gate".to_string(),
            ));
        }
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
        if !self.cluster_mesh_enabled.load(Ordering::Acquire) {
            return Err(MeshRequestError::Reverse(
                "reverse relay is disabled by the cluster Mesh gate".to_string(),
            ));
        }
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

    /// Sends only through the Raft-assigned Reverse route for control-plane callers that opt in.
    /// History repository direct requests intentionally do not use this method.
    #[allow(dead_code)]
    pub(crate) async fn send_peer_reverse_request(
        &self,
        peer: &MeshPeerTarget,
        request: MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
    ) -> Result<reqwest::Response, MeshRequestError> {
        if !self.cluster_mesh_enabled.load(Ordering::Acquire) {
            return Err(MeshRequestError::Reverse(
                "reverse relay is disabled by the cluster Mesh gate".to_string(),
            ));
        }
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
            let mesh_epoch = self.cluster_mesh_epoch.load(Ordering::Acquire);
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
                    self.record_reverse_sample(peer, started, &request, &candidate, mesh_epoch)
                        .await;
                    return Ok(response);
                }
                Err(
                    error @ (MeshRequestError::OutcomeUnknown | MeshRequestError::TransportTimeout),
                ) if !request.allow_ambiguous_fallback => {
                    return Err(error);
                }
                Err(error @ MeshRequestError::ReverseTimeout)
                    if !request.allow_ambiguous_fallback =>
                {
                    return Err(error);
                }
                Err(error @ (MeshRequestError::Auth(_) | MeshRequestError::Protocol(_))) => {
                    return Err(error);
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            MeshRequestError::Reverse("reverse assignment has no usable candidate".into())
        }))
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn send_outer_request(
    client: &reqwest::Client,
    request: &MeshRequest,
    url: &str,
    headers: &axum::http::HeaderMap,
    budget: Duration,
    allow_ambiguous_fallback: bool,
    cluster_mesh_enabled: &Arc<AtomicBool>,
    mesh_gate_lock: &Arc<tokio::sync::RwLock<()>>,
) -> Result<reqwest::Response, MeshRequestError> {
    let gate_guard = mesh_gate_lock.clone().read_owned().await;
    if !cluster_mesh_enabled.load(Ordering::Acquire) {
        return Err(MeshRequestError::Reverse(
            "cluster Mesh gate is disabled".to_string(),
        ));
    }
    let mut builder = client
        .request(request.method.clone(), url)
        .body(request.body.clone());
    for (name, value) in headers {
        builder = builder.header(name, value);
    }
    match tokio::time::timeout(budget, builder.send()).await {
        Ok(result) => result
            .map(|response| attach_mesh_gate(response, gate_guard))
            .map_err(|error| public_transport_error(error, allow_ambiguous_fallback)),
        Err(_) if allow_ambiguous_fallback => Err(MeshRequestError::ReverseTimeout),
        Err(_) => Err(MeshRequestError::TransportTimeout),
    }
}
