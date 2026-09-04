use axum::{Extension, Json};
use chrono::Utc;
use serde::Deserialize;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InternalRestoreNodeRequest {
    node_id: String,
    #[serde(default)]
    allow_missing_node_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InternalRestoreStaleLearnerRequest {
    node_id: String,
    #[serde(default)]
    apply: bool,
    #[serde(default)]
    expected_membership: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct InternalRestoreStaleLearnerResponse {
    dry_run: bool,
    node_id: String,
    raft_node_id: u64,
    expected_membership: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation: Option<crate::state::MembershipOperation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InternalPruneAbsentNodeRequest {
    node_id: String,
    #[serde(default)]
    delete_endpoints: bool,
    #[serde(default)]
    expected_endpoint_ids: Vec<String>,
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
    let allowed_missing = request
        .allow_missing_node_ids
        .iter()
        .map(|node_id| {
            raft_node_id_from_ulid(node_id)
                .map_err(|error| ApiError::invalid_request(error.to_string()))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if allowed_missing.len() != request.allow_missing_node_ids.len() {
        return Err(ApiError::invalid_request(
            "restore allowlist must not contain duplicate node IDs",
        ));
    }
    if allowed_missing.contains(&raft_node_id) {
        return Err(ApiError::invalid_request(
            "restore allowlist must not contain the target node",
        ));
    }

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
                &allowed_missing,
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
                remove_learner: false,
                delete_endpoints: false,
                expected_endpoint_ids: Vec::new(),
                expected_endpoint_tags: Vec::new(),
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

/// Adopt one exact stale learner into the existing durable Restore lifecycle. This is separate
/// from ordinary absent-node restore so no caller can use it as a generic learner-promotion API.
pub(super) async fn admin_internal_restore_stale_learner(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<InternalRestoreStaleLearnerRequest>,
) -> Result<Json<InternalRestoreStaleLearnerResponse>, ApiError> {
    if internal.is_none() {
        return Err(ApiError::unauthorized("internal auth required"));
    }
    let preview =
        crate::raft_membership_guard::stale_learner_recovery::preview_stale_learner_recovery(
            state.raft.clone(),
            state.store.clone(),
            &request.node_id,
        )
        .await
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    join_capability::require_membership_lifecycle_on_voters(&state).await?;

    if !request.apply {
        return Ok(Json(InternalRestoreStaleLearnerResponse {
            dry_run: true,
            node_id: preview.node_id,
            raft_node_id: preview.raft_node_id,
            expected_membership: preview.expected_membership,
            operation: None,
        }));
    }

    let expected_membership = request
        .expected_membership
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::invalid_request("--apply requires expected_membership"))?;
    let operation =
        crate::raft_membership_guard::stale_learner_recovery::begin_stale_learner_recovery(
            state.raft.clone(),
            state.store.clone(),
            &request.node_id,
            &expected_membership,
        )
        .await
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    crate::raft_membership_guard::resume_membership_operations_once(
        state.raft.clone(),
        state.store.clone(),
    )
    .await
    .map_err(|error| {
        ApiError::internal(format!("stale learner recovery resume failed: {error}"))
    })?;
    let operation = state
        .store
        .lock()
        .await
        .state()
        .membership_operations
        .get(&operation.operation_id)
        .cloned()
        .ok_or_else(|| ApiError::internal("stale learner recovery operation disappeared"))?;
    Ok(Json(InternalRestoreStaleLearnerResponse {
        dry_run: false,
        node_id: preview.node_id,
        raft_node_id: preview.raft_node_id,
        expected_membership: operation.expected_membership.clone(),
        operation: Some(operation),
    }))
}

/// Remove a DesiredState node that is already absent from Raft membership after an explicitly
/// authorized single-node recovery. This is intentionally narrower than normal node deletion:
/// it cannot remove a live voter or learner, and the endpoint snapshot must match exactly.
pub(super) async fn admin_internal_prune_absent_node(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<InternalPruneAbsentNodeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if internal.is_none() {
        return Err(ApiError::unauthorized("internal auth required"));
    }
    join_capability::require_membership_lifecycle_on_voters(&state).await?;

    let raft_node_id = raft_node_id_from_ulid(&request.node_id)
        .map_err(|error| ApiError::invalid_request(error.to_string()))?;
    let (node_exists, actual_endpoint_ids, actual_endpoint_tags) = {
        let store = state.store.lock().await;
        let node_exists = store.get_node(&request.node_id).is_some();
        let endpoints = store
            .list_endpoints()
            .into_iter()
            .filter(|endpoint| endpoint.node_id == request.node_id)
            .collect::<Vec<_>>();
        (
            node_exists,
            endpoints
                .iter()
                .map(|endpoint| endpoint.endpoint_id.clone())
                .collect::<BTreeSet<_>>(),
            endpoints
                .iter()
                .map(|endpoint| endpoint.tag.clone())
                .collect::<Vec<_>>(),
        )
    };
    if !node_exists {
        return Err(ApiError::not_found(format!(
            "node not found: {}",
            request.node_id
        )));
    }
    let expected_endpoint_ids = request
        .expected_endpoint_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if actual_endpoint_ids != expected_endpoint_ids
        || (!request.delete_endpoints && !actual_endpoint_ids.is_empty())
    {
        return Err(ApiError::conflict(
            crate::domain::DomainError::NodeEndpointSetChanged {
                node_id: request.node_id.clone(),
            }
            .to_string(),
        ));
    }

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
    if metrics
        .membership_config
        .membership()
        .get_node(&raft_node_id)
        .is_some()
    {
        return Err(ApiError::conflict(
            "absent-node prune requires the target to be absent from Raft membership",
        ));
    }
    if state
        .store
        .lock()
        .await
        .state()
        .active_membership_operation()
        .is_some()
    {
        return Err(ApiError::conflict(
            "another membership lifecycle operation is active",
        ));
    }

    let operation = crate::state::MembershipOperation {
        operation_id: uuid::Uuid::new_v4().to_string(),
        kind: crate::state::MembershipOperationKind::RemoveNode,
        raft_node_id,
        node_id: Some(request.node_id.clone()),
        expected_membership: crate::raft_membership_guard::membership_revision(&metrics)
            .map_err(|error| ApiError::internal(error.to_string()))?,
        phase: crate::state::MembershipOperationPhase::Prepared,
        legacy: false,
        remove_learner: false,
        delete_endpoints: request.delete_endpoints,
        expected_endpoint_ids: request.expected_endpoint_ids.clone(),
        expected_endpoint_tags: actual_endpoint_tags,
        created_at: Utc::now().to_rfc3339(),
        next_retry_at: None,
        terminal_at: None,
        evidence: Some("authorized prune of node absent after single-node recovery".to_string()),
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
    drop(guard);

    for _ in 0..3 {
        crate::raft_membership_guard::resume_membership_operations_once(
            state.raft.clone(),
            state.store.clone(),
        )
        .await
        .map_err(|error| ApiError::internal(format!("absent-node prune resume failed: {error}")))?;
    }
    let cleanup = membership_removal_cleanup(&state);
    let _ = crate::raft_membership_guard::finalize_remove_node_cleanup_once(
        state.raft.clone(),
        state.store.clone(),
        &cleanup,
    )
    .await;
    let phase = state
        .store
        .lock()
        .await
        .state()
        .membership_operations
        .get(&operation.operation_id)
        .map(|operation| operation.phase.clone())
        .unwrap_or(crate::state::MembershipOperationPhase::Prepared);
    Ok(Json(serde_json::json!({
        "ok": phase == crate::state::MembershipOperationPhase::Completed,
        "operation_id": operation.operation_id,
        "phase": phase,
    })))
}
