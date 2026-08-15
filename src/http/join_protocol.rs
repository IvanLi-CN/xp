use chrono::{DateTime, Utc};

use super::{ApiError, AppState, ClusterJoinRequest};

pub(super) struct BootstrapReservation {
    pub existing: Option<crate::join_session::JoinSession>,
    pub activation_deadline: String,
    pub signed_cert_pem: String,
}

pub(super) async fn resolve_reservation(
    state: &AppState,
    req: &ClusterJoinRequest,
    token: &crate::cluster_identity::JoinToken,
    request_fingerprint: &str,
    ca_key_pem: &str,
) -> Result<BootstrapReservation, ApiError> {
    let existing = state
        .store
        .lock()
        .await
        .state()
        .join_sessions
        .get(&token.token_id)
        .cloned();
    if existing.is_none() {
        token
            .validate_at(Utc::now())
            .map_err(|e| ApiError::invalid_request(e.to_string()))?;
    }
    let activation_deadline = existing
        .as_ref()
        .map(|session| session.activation_deadline.clone())
        .unwrap_or_else(|| {
            (Utc::now()
                + chrono::Duration::from_std(crate::join_session::ACTIVATION_TIMEOUT)
                    .expect("join activation timeout fits chrono duration"))
            .to_rfc3339()
        });
    let signed_cert_pem = if let Some(session) = existing.as_ref() {
        if session.request_fingerprint != request_fingerprint {
            return Err(ApiError::invalid_request(
                "join token reserved by another request",
            ));
        }
        if session.status == crate::join_session::JoinSessionStatus::Expired {
            return Err(ApiError::invalid_request("join token already used"));
        }
        if session.status.is_pending()
            && DateTime::parse_from_rfc3339(&session.activation_deadline)
                .map_err(|error| ApiError::internal(error.to_string()))?
                <= Utc::now()
        {
            return Err(ApiError::invalid_request(
                "join activation deadline has expired",
            ));
        }
        session.signed_cert_pem.clone()
    } else {
        if state.store.lock().await.get_node(&token.token_id).is_some() {
            return Err(ApiError::invalid_request("join token already used"));
        }
        crate::cluster_identity::sign_node_csr(&state.cluster.cluster_id, ca_key_pem, &req.csr_pem)
            .map_err(|e| ApiError::internal(e.to_string()))?
    };
    Ok(BootstrapReservation {
        existing,
        activation_deadline,
        signed_cert_pem,
    })
}
