use axum::{Extension, Json};
use chrono::Utc;
use serde::Deserialize;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InternalRestoreNodeRequest {
    node_id: String,
}

/// Restore has no raw membership escape hatch: it only creates or resumes the durable operation
/// that the leader-side lifecycle coordinator is allowed to advance.
pub(super) async fn admin_internal_restore_node(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<InternalRestoreNodeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if internal.is_none() {
        return Err(ApiError::unauthorized("internal auth required"));
    }
    join_capability::require_membership_lifecycle_on_voters(&state).await?;

    let node = state
        .store
        .lock()
        .await
        .get_node(&request.node_id)
        .ok_or_else(|| ApiError::not_found(format!("node not found: {}", request.node_id)))?;
    let raft_node_id = raft_node_id_from_ulid(&node.node_id)
        .map_err(|error| ApiError::invalid_request(error.to_string()))?;

    let guard = crate::raft_membership_guard::membership_operation_gate()
        .lock_owned()
        .await;
    state
        .raft
        .ensure_linearizable()
        .await
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    let metrics = raft_metrics(&state);
    if !is_leader(&metrics) {
        return Err(ApiError::invalid_request("not leader"));
    }

    let target_is_voter = metrics
        .membership_config
        .membership()
        .voter_ids()
        .any(|node_id| node_id == raft_node_id);
    let active_operation = {
        let store = state.store.lock().await;
        store.state().active_membership_operation().cloned()
    };
    let operation = match active_operation {
        Some(operation)
            if operation.kind == crate::state::MembershipOperationKind::Restore
                && operation.raft_node_id == raft_node_id =>
        {
            operation
        }
        Some(_) => {
            return Err(ApiError::conflict(
                "another membership lifecycle operation is active",
            ));
        }
        None if target_is_voter => {
            return Ok(Json(serde_json::json!({
                "ok": true,
                "already_voter": true,
            })));
        }
        None => {
            crate::raft_membership_guard::require_clean_membership_for_restore_node(
                state.raft.clone(),
                state.store.clone(),
                raft_node_id,
            )
            .await
            .map_err(|error| ApiError::conflict(error.to_string()))?;
            let operation = crate::state::MembershipOperation {
                operation_id: uuid::Uuid::new_v4().to_string(),
                kind: crate::state::MembershipOperationKind::Restore,
                raft_node_id,
                node_id: Some(node.node_id),
                expected_membership: crate::raft_membership_guard::membership_revision(&metrics)
                    .map_err(|error| ApiError::internal(error.to_string()))?,
                phase: crate::state::MembershipOperationPhase::Prepared,
                legacy: false,
                delete_endpoints: false,
                expected_endpoint_ids: Vec::new(),
                created_at: Utc::now().to_rfc3339(),
                next_retry_at: None,
                terminal_at: None,
                evidence: Some("restore requested".to_string()),
            };
            let _ = raft_write(
                &state,
                crate::state::DesiredStateCommand::BeginMembershipOperation {
                    operation: Box::new(operation.clone()),
                    node: None,
                    join_session: None,
                },
            )
            .await?;
            operation
        }
    };
    drop(guard);

    crate::raft_membership_guard::resume_membership_operations_once(
        state.raft.clone(),
        state.store.clone(),
    )
    .await
    .map_err(|error| ApiError::internal(format!("restore resume failed: {error}")))?;
    let resumed = state
        .store
        .lock()
        .await
        .state()
        .membership_operations
        .get(&operation.operation_id)
        .cloned()
        .ok_or_else(|| ApiError::internal("restore operation disappeared"))?;
    Ok(Json(serde_json::json!({
        "ok": resumed.phase == crate::state::MembershipOperationPhase::Completed,
        "already_voter": false,
        "operation_id": resumed.operation_id,
        "phase": resumed.phase,
    })))
}
