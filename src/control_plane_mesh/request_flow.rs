use super::*;

impl MeshAwareHttpClient {
    /// Sends a signed public health probe without functional telemetry, breaker, active-path or
    /// direct-validation side effects.
    pub(crate) async fn send_public_probe(
        &self,
        peer: &MeshPeerTarget,
        request: &MeshRequest,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
    ) -> Result<reqwest::Response, MeshRequestError> {
        let url = join_url(&peer.public_base_url, &request.path_and_query, false)?;
        let deadline = Instant::now() + request.total_budget;
        self.send_public_signed(
            &url,
            request,
            &peer.node_id,
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
            deadline,
            false,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn send_public_signed(
        &self,
        url: &str,
        request: &MeshRequest,
        target_id: &str,
        cluster_ca_key_pem: &str,
        cluster_ca_cert_pem: &str,
        deadline: Instant,
        allow_unsigned_not_found: bool,
    ) -> Result<reqwest::Response, MeshRequestError> {
        let (response, verified) = retry::signed_send_with_public_gateway_retries_until(
            &self.public_direct,
            url,
            request,
            target_id,
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
            deadline,
            true,
        )
        .await?;
        let Some(acknowledgement) = response.headers().get(internal_auth::INTERNAL_ACK_HEADER)
        else {
            if allow_unsigned_not_found && response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(response);
            }
            return Err(MeshRequestError::Protocol(
                "public response has no signed acknowledgement".to_string(),
            ));
        };
        let ack = acknowledgement.to_str().map_err(|_| {
            MeshRequestError::Protocol(
                "public response carries a malformed signed acknowledgement".to_string(),
            )
        })?;
        if let Err(error) = internal_auth::verify_ack_v2(
            cluster_ca_key_pem,
            cluster_ca_cert_pem,
            &verified,
            target_id,
            response.status().as_u16(),
            ack,
        ) {
            return Err(error.into());
        }
        Ok(response)
    }
}

pub(super) fn direct_mesh_is_eligible(
    peer: &MeshPeerTarget,
    cluster_mesh_enabled: bool,
    validation: DirectValidationState,
) -> bool {
    peer.mesh_base_url.is_some()
        && cluster_mesh_enabled
        && validation == DirectValidationState::Verified
}
