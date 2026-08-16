use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{
    node_history::NodeHistoryHandle,
    raft::{app::RaftFacade, types::NodeId},
    raft_membership_guard::{
        block_membership_operation, membership_operation_gate, require_expected_membership,
        transition_operation,
    },
    reconcile::ReconcileHandle,
    state::{MembershipOperationKind, MembershipOperationPhase},
};

#[derive(Clone)]
pub struct MembershipRemovalCleanup {
    pub local_raft_node_id: NodeId,
    pub local_node_id: String,
    pub node_history: NodeHistoryHandle,
    pub reconcile: ReconcileHandle,
}

/// Finish only the non-Raft effects of an already committed remove-node operation. The operation
/// remains `MembershipRemoved` until these idempotent requests have been issued, so a crash never
/// reports a deletion complete before its runtime and history cleanup is recoverable.
pub async fn finalize_remove_node_cleanup_once(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<crate::state::JsonSnapshotStore>>,
    cleanup: &MembershipRemovalCleanup,
) -> anyhow::Result<bool> {
    let Ok(_guard) = membership_operation_gate().try_lock_owned() else {
        return Ok(false);
    };
    let operation = store
        .lock()
        .await
        .state()
        .active_membership_operation()
        .cloned();
    let Some(operation) = operation else {
        return Ok(false);
    };
    if operation.kind != MembershipOperationKind::RemoveNode
        || operation.phase != MembershipOperationPhase::MembershipRemoved
    {
        return Ok(false);
    }
    let node_id = operation
        .node_id
        .clone()
        .ok_or_else(|| anyhow::anyhow!("remove-node operation is missing node_id"))?;
    if store.lock().await.get_node(&node_id).is_some() {
        return Ok(false);
    }

    let metrics = raft.metrics().borrow().clone();
    if metrics.current_leader != Some(cleanup.local_raft_node_id) {
        return Ok(false);
    }
    if let Err(error) = require_expected_membership(&raft, &operation).await {
        block_membership_operation(&raft, operation, error.to_string()).await?;
        return Ok(false);
    }

    for tag in &operation.expected_endpoint_tags {
        cleanup.reconcile.request_remove_inbound(tag.clone());
    }
    cleanup.node_history.clear_node(&node_id).await;
    let cleanup_nodes = store.lock().await.list_nodes();
    for destination in cleanup_nodes {
        if destination.node_id != cleanup.local_node_id && destination.node_id != node_id {
            cleanup
                .node_history
                .queue_node_history_cleanup(&destination.node_id, &node_id)
                .await;
        }
    }
    cleanup.reconcile.request_full();
    let _ = transition_operation(
        &raft,
        operation,
        MembershipOperationPhase::Completed,
        "desired node deleted and cleanup requested",
    )
    .await?;
    Ok(true)
}
