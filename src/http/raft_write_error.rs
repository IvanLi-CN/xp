use axum::http::StatusCode;

use crate::raft::app::{RaftWriteDispatchState, RaftWriteError};

use super::ApiError;

pub(super) fn map_raft_write_error(error: anyhow::Error) -> ApiError {
    let Some(error) = error.downcast_ref::<RaftWriteError>() else {
        return ApiError::new(
            "raft_write_outcome_unknown",
            StatusCode::BAD_GATEWAY,
            "write result could not be verified; it may already be applied",
        )
        .with_detail("failure_layer", "raft_write")
        .with_detail("cause", "unclassified_raft_error")
        .with_detail("confidence", "unknown")
        .with_detail("attempted_path", "unknown")
        .with_detail("dispatch_state", "unknown")
        .with_detail("retryable", true)
        .with_detail("manual_retry_warning", "may_overwrite_concurrent_changes")
        .with_detail("support_id", crate::id::new_ulid_string());
    };

    let (code, status, message) = match error.dispatch_state() {
        RaftWriteDispatchState::NotDispatched => (
            "raft_write_not_dispatched",
            StatusCode::SERVICE_UNAVAILABLE,
            "write was not sent to the leader",
        ),
        RaftWriteDispatchState::DispatchedNoVerifiedResponse | RaftWriteDispatchState::Unknown => (
            "raft_write_outcome_unknown",
            StatusCode::BAD_GATEWAY,
            "write result could not be verified; it may already be applied",
        ),
    };
    let mut api_error = ApiError::new(code, status, message)
        .with_detail("failure_layer", "raft_write")
        .with_detail("cause", error.cause())
        .with_detail("confidence", error.confidence())
        .with_detail("attempted_path", "mesh_or_public")
        .with_detail("dispatch_state", error.dispatch_state().as_str())
        .with_detail("retryable", true)
        .with_detail("support_id", error.support_id());
    if let Some(target_node_id) = error.target_node_id() {
        api_error = api_error.with_detail("target_node_id", target_node_id);
    }
    if let Some(remote_status) = error.remote_status() {
        api_error = api_error.with_detail("remote_status", remote_status);
    }
    if error.dispatch_state() != RaftWriteDispatchState::NotDispatched {
        api_error =
            api_error.with_detail("manual_retry_warning", "may_overwrite_concurrent_changes");
    }
    api_error
}
