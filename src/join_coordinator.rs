use std::{collections::BTreeSet, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

use crate::{
    join_session::JoinSessionStatus,
    raft::{app::RaftFacade, types::raft_node_id_from_ulid},
    state::{DesiredStateApplyResult, DesiredStateCommand, JsonSnapshotStore},
};

async fn write_applied(
    raft: &Arc<dyn RaftFacade>,
    command: DesiredStateCommand,
) -> anyhow::Result<DesiredStateApplyResult> {
    match raft.client_write(command).await? {
        crate::raft::types::ClientResponse::Ok { result } => Ok(result),
        crate::raft::types::ClientResponse::Err { code, message, .. } => {
            anyhow::bail!("join coordinator state write rejected: {code}: {message}")
        }
    }
}

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
            let node = store
                .lock()
                .await
                .get_node(&session.node_id)
                .ok_or_else(|| anyhow::anyhow!("pending join node is missing"))?;
            write_applied(
                &raft,
                DesiredStateCommand::UpsertNode {
                    node,
                    join_session: Some(session),
                },
            )
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
            session.status = JoinSessionStatus::Expired;
            session.terminal_at = Some(Utc::now().to_rfc3339());
            let expected_endpoint_ids = store
                .lock()
                .await
                .list_endpoints()
                .into_iter()
                .filter(|endpoint| endpoint.node_id == session.node_id)
                .map(|endpoint| endpoint.endpoint_id)
                .collect();
            write_applied(
                &raft,
                DesiredStateCommand::DeleteNode {
                    node_id: session.node_id.clone(),
                    delete_endpoints: true,
                    expected_endpoint_ids,
                    join_session: Some(session),
                },
            )
            .await?;
            continue;
        }
        if session.status == JoinSessionStatus::Reserved {
            let registered = metrics
                .membership_config
                .nodes()
                .any(|(member_id, _)| *member_id == node_id);
            if !registered {
                continue;
            }
            session.status = JoinSessionStatus::LearnerRegistered;
            let node = store
                .lock()
                .await
                .get_node(&session.node_id)
                .ok_or_else(|| anyhow::anyhow!("reserved join node is missing"))?;
            write_applied(
                &raft,
                DesiredStateCommand::UpsertNode {
                    node,
                    join_session: Some(session.clone()),
                },
            )
            .await?;
        }
        let _guard = crate::raft_membership_guard::membership_operation_gate()
            .lock_owned()
            .await;
        if raft
            .wait_learner_caught_up(node_id, session.required_log_index, Duration::from_secs(5))
            .await
            .is_ok()
        {
            if deadline <= Utc::now() {
                continue;
            }
            raft.add_voters(BTreeSet::from([node_id])).await?;
            session.status = JoinSessionStatus::Consumed;
            session.terminal_at = Some(Utc::now().to_rfc3339());
            let node = store
                .lock()
                .await
                .get_node(&session.node_id)
                .ok_or_else(|| anyhow::anyhow!("pending join node is missing"))?;
            write_applied(
                &raft,
                DesiredStateCommand::UpsertNode {
                    node,
                    join_session: Some(session),
                },
            )
            .await?;
        }
    }
    Ok(())
}
