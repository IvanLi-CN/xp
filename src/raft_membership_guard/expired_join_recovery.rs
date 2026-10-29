use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{
    join_session::JoinSessionStatus,
    raft::app::RaftFacade,
    state::{MembershipOperation, MembershipOperationPhase},
};

use super::{block_membership_operation, is_current_local_leader, transition_operation};

// Recover only the terminal postcondition of a Join cleanup; never infer a membership removal.
pub(super) async fn resume_expired_join_operation(
    raft: &Arc<dyn RaftFacade>,
    store: &Arc<Mutex<crate::state::JsonSnapshotStore>>,
    operation: MembershipOperation,
) -> anyhow::Result<()> {
    let node_id = operation
        .node_id
        .clone()
        .ok_or_else(|| anyhow::anyhow!("join operation is missing node_id"))?;
    let metrics = raft.metrics().borrow().clone();
    if !is_current_local_leader(&metrics) {
        return Ok(());
    }
    raft.ensure_linearizable().await?;
    let metrics = raft.metrics().borrow().clone();
    if !is_current_local_leader(&metrics) {
        return Ok(());
    }
    if metrics
        .membership_config
        .membership()
        .get_joint_config()
        .len()
        > 1
    {
        return Ok(());
    }

    let (session, node_exists, endpoint_exists) = {
        let store = store.lock().await;
        (
            store.state().join_sessions.get(&node_id).cloned(),
            store.get_node(&node_id).is_some(),
            store
                .list_endpoints()
                .into_iter()
                .any(|endpoint| endpoint.node_id == node_id),
        )
    };
    let Some(session) = session else {
        return Ok(());
    };
    if session.status != JoinSessionStatus::Expired || session.terminal_at.is_none() {
        return Ok(());
    }

    let target_exists = metrics
        .membership_config
        .nodes()
        .any(|(node_id, _)| *node_id == operation.raft_node_id);
    if target_exists {
        block_membership_operation(
            raft,
            operation,
            "join expired while its learner remained; explicit stale-learner recovery is required",
        )
        .await?;
        return Ok(());
    }
    if node_exists || endpoint_exists {
        return Ok(());
    }

    transition_operation(
        raft,
        operation,
        MembershipOperationPhase::Expired,
        "expired join operation recovered after its learner and desired state were removed",
    )
    .await?;
    Ok(())
}
