use super::*;

impl MeshAwareHttpClient {
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
