use std::{collections::BTreeSet, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

use crate::{
    join_session::JoinSessionStatus,
    raft::{app::RaftFacade, types::raft_node_id_from_ulid},
    state::{DesiredStateCommand, JsonSnapshotStore},
};

pub fn spawn_join_coordinator(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<JsonSnapshotStore>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            if let Err(error) = reconcile_once(raft.clone(), store.clone()).await {
                tracing::warn!(%error, "join coordinator reconcile failed");
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })
}

async fn reconcile_once(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<JsonSnapshotStore>>,
) -> anyhow::Result<()> {
    let metrics = raft.metrics().borrow().clone();
    if metrics.state != openraft::ServerState::Leader {
        return Ok(());
    }
    let sessions = store
        .lock()
        .await
        .state()
        .join_sessions
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for mut session in sessions {
        if !session.status.is_pending() {
            continue;
        }
        let node_id = raft_node_id_from_ulid(&session.node_id)?;
        let voters = metrics
            .membership_config
            .membership()
            .voter_ids()
            .collect::<BTreeSet<_>>();
        if voters.contains(&node_id) {
            session.status = JoinSessionStatus::Consumed;
            session.terminal_at = Some(Utc::now().to_rfc3339());
            raft.client_write(DesiredStateCommand::UpsertJoinSession { session })
                .await?;
            continue;
        }
        let deadline =
            DateTime::parse_from_rfc3339(&session.activation_deadline)?.with_timezone(&Utc);
        if deadline <= Utc::now() {
            let _guard = crate::raft_membership_guard::membership_operation_gate()
                .lock_owned()
                .await;
            let _ = raft
                .change_membership(
                    openraft::ChangeMembers::RemoveVoters(BTreeSet::from([node_id])),
                    true,
                )
                .await;
            raft.change_membership(
                openraft::ChangeMembers::RemoveNodes(BTreeSet::from([node_id])),
                true,
            )
            .await?;
            raft.client_write(DesiredStateCommand::DeleteNode {
                node_id: session.node_id.clone(),
                delete_endpoints: false,
                expected_endpoint_ids: Vec::new(),
            })
            .await?;
            session.status = JoinSessionStatus::Expired;
            session.terminal_at = Some(Utc::now().to_rfc3339());
            raft.client_write(DesiredStateCommand::UpsertJoinSession { session })
                .await?;
            continue;
        }
        if session.status == JoinSessionStatus::Reserved {
            continue;
        }
        let _guard = crate::raft_membership_guard::membership_operation_gate()
            .lock_owned()
            .await;
        if raft
            .wait_learner_caught_up(node_id, session.required_log_index, Duration::from_secs(5))
            .await
            .is_ok()
        {
            raft.add_voters(BTreeSet::from([node_id])).await?;
            session.status = JoinSessionStatus::Consumed;
            session.terminal_at = Some(Utc::now().to_rfc3339());
            raft.client_write(DesiredStateCommand::UpsertJoinSession { session })
                .await?;
        }
    }
    Ok(())
}
