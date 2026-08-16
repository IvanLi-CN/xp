use std::collections::BTreeSet;

use chrono::{DateTime, Utc};

use super::{ApiError, AppState, ClusterJoinRequest, raft_metrics, raft_write};

pub(super) async fn bootstrap_sender_ids(state: &AppState) -> Vec<String> {
    let voter_ids = raft_metrics(state)
        .membership_config
        .membership()
        .voter_ids()
        .collect::<BTreeSet<_>>();
    let store = state.store.lock().await;
    let mut sender_ids = store
        .list_nodes()
        .into_iter()
        .filter_map(|node| {
            crate::raft::types::raft_node_id_from_ulid(&node.node_id)
                .ok()
                .filter(|raft_id| voter_ids.contains(raft_id))
                .map(|_| node.node_id)
        })
        .collect::<BTreeSet<_>>();
    sender_ids.insert(state.cluster.node_id.clone());
    sender_ids.into_iter().collect()
}

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

pub(super) async fn mark_learner_registered(
    state: &AppState,
    node: crate::domain::Node,
) -> Result<(), ApiError> {
    let session = {
        let store = state.store.lock().await;
        let session = store
            .state()
            .join_sessions
            .get(&node.node_id)
            .cloned()
            .expect("join reservation was committed");
        let required_log_index = raft_metrics(state)
            .last_log_index
            .unwrap_or(0)
            .max(session.required_log_index);
        session.learner_registered(required_log_index)
    };
    let _ = raft_write(
        state,
        crate::state::DesiredStateCommand::UpsertNode {
            node,
            join_session: Some(session),
        },
    )
    .await?;
    Ok(())
}
