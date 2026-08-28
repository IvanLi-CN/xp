use super::*;

#[derive(Debug, Clone)]
pub(super) struct LocalReverseRelay {
    pub(super) node_id: String,
    pub(super) base_url: String,
}

impl MeshAwareHttpClient {
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
