use crate::{
    domain::Node,
    join_session::JoinSession,
    state::{
        DesiredStateApplyResult, PersistedState, StoreError, sync_node_user_endpoint_memberships,
        validate_node_quota_config, validate_node_quota_reset,
    },
};

pub(super) fn apply_upsert_node(
    state: &mut PersistedState,
    node: &Node,
    session: Option<&JoinSession>,
) -> Result<DesiredStateApplyResult, StoreError> {
    validate_session(state, session)?;
    validate_node_quota_reset(&node.quota_reset)?;
    validate_node_quota_config(node)?;
    state.nodes.insert(node.node_id.clone(), node.clone());
    commit_session(state, session);
    sync_node_user_endpoint_memberships(state);
    Ok(DesiredStateApplyResult::Applied)
}

pub(super) fn validate_session(
    state: &PersistedState,
    session: Option<&JoinSession>,
) -> Result<(), StoreError> {
    if let Some(session) = session
        && let Some(current) = state.join_sessions.get(&session.node_id)
    {
        current
            .validate_successor(session)
            .map_err(|message| StoreError::InvalidJoinSession { message })?;
    }
    Ok(())
}

pub(super) fn commit_session(state: &mut PersistedState, session: Option<&JoinSession>) {
    if let Some(session) = session {
        state
            .join_sessions
            .insert(session.node_id.clone(), session.clone());
    }
}
