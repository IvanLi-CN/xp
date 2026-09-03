use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{
    raft::{app::RaftFacade, types::NodeId},
    state::JsonSnapshotStore,
};

/// Check the narrower preconditions for registering a fresh learner. An unrelated learner is an
/// observable incident, but it does not own this join and therefore cannot veto it. The target
/// identity, active lifecycle operation, joint consensus state, and all voter/DesiredState
/// mapping invariants remain blocking preconditions.
pub async fn require_fresh_join_admission(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<JsonSnapshotStore>>,
    target_node_id: &str,
    target_raft_node_id: NodeId,
) -> anyhow::Result<()> {
    raft.ensure_linearizable().await?;
    let metrics = raft.metrics().borrow().clone();
    let membership = metrics.membership_config.membership();
    if membership.get_joint_config().len() > 1 {
        anyhow::bail!("fresh join is forbidden during joint consensus")
    }
    if membership.get_node(&target_raft_node_id).is_some() {
        anyhow::bail!(
            "fresh join target is already a Raft member: raft_node_id={target_raft_node_id}"
        )
    }

    let has_target_node = {
        let store = store.lock().await;
        if store.state().active_membership_operation().is_some() {
            anyhow::bail!("fresh join is blocked by an active membership operation")
        }
        store.state().nodes.contains_key(target_node_id)
    };
    if has_target_node {
        anyhow::bail!("fresh join target already exists in DesiredState: node_id={target_node_id}")
    }

    let audit = crate::raft_membership_guard::audit_membership(raft, store).await;
    let mut admission_audit = audit.clone();
    admission_audit.unexpected_learners.clear();
    if !audit.unexpected_learners.is_empty() {
        tracing::warn!(
            learners = ?audit.unexpected_learners,
            target_node_id,
            "fresh join admitted while unrelated learners remain under incident observation"
        );
    }
    admission_audit
        .is_clean()
        .then_some(())
        .ok_or_else(|| crate::raft_membership_guard::membership_invariant_error(&admission_audit))
}
