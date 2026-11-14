use crate::{
    join_session::{JoinSession, JoinSessionStatus},
    raft::app::RaftFacade,
    state::{JsonSnapshotStore, MembershipOperationKind, MembershipOperationPhase},
};
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::write_applied;

pub(super) async fn transition_join_operation(
    raft: &Arc<dyn RaftFacade>,
    store: &Arc<Mutex<JsonSnapshotStore>>,
    node_id: u64,
    phase: MembershipOperationPhase,
    evidence: &str,
) -> anyhow::Result<()> {
    let Some(mut operation) = store
        .lock()
        .await
        .state()
        .active_membership_operation()
        .filter(|operation| {
            operation.raft_node_id == node_id && operation.kind == MembershipOperationKind::Join
        })
        .cloned()
    else {
        return Ok(());
    };
    raft.ensure_linearizable().await?;
    operation.expected_membership =
        crate::raft_membership_guard::membership_revision(&raft.metrics().borrow().clone())?;
    operation.phase = phase;
    operation.evidence = Some(evidence.to_string());
    if operation.phase.is_terminal() {
        operation.terminal_at = Some(Utc::now().to_rfc3339());
    }
    write_applied(
        raft,
        crate::state::DesiredStateCommand::TransitionMembershipOperation { operation },
    )
    .await?;
    Ok(())
}

pub(super) async fn complete_consumed_join(
    raft: &Arc<dyn RaftFacade>,
    store: &Arc<Mutex<JsonSnapshotStore>>,
    session: &JoinSession,
    node_id: u64,
    is_voter: bool,
) -> anyhow::Result<()> {
    if session.status != JoinSessionStatus::Consumed || !is_voter {
        return Ok(());
    }
    let Some(operation) = store
        .lock()
        .await
        .state()
        .active_membership_operation()
        .filter(|operation| {
            operation.kind == MembershipOperationKind::Join
                && operation.raft_node_id == node_id
                && operation.node_id.as_deref() == Some(session.node_id.as_str())
                && operation.phase == MembershipOperationPhase::VoterPromoted
        })
        .cloned()
    else {
        return Ok(());
    };
    transition_join_operation(
        raft,
        store,
        node_id,
        MembershipOperationPhase::Completed,
        "join completion recovered after consumed session",
    )
    .await?;
    tracing::info!(
        node_id = %session.node_id,
        operation_id = %operation.operation_id,
        "recovered completed join membership operation"
    );
    Ok(())
}
