use std::{collections::BTreeSet, sync::Arc, sync::OnceLock, time::Duration};

use anyhow::Context;
use tokio::sync::Mutex;

use crate::raft::{
    app::RaftFacade,
    types::{NodeId, NodeMeta},
};

static MEMBERSHIP_OPERATION_GATE: OnceLock<Arc<Mutex<()>>> = OnceLock::new();

pub fn membership_operation_gate() -> Arc<Mutex<()>> {
    MEMBERSHIP_OPERATION_GATE
        .get_or_init(|| Arc::new(Mutex::new(())))
        .clone()
}

pub fn non_voter_membership_node_ids(
    metrics: &openraft::RaftMetrics<NodeId, NodeMeta>,
) -> BTreeSet<NodeId> {
    let voters = metrics
        .membership_config
        .membership()
        .voter_ids()
        .collect::<BTreeSet<_>>();

    metrics
        .membership_config
        .nodes()
        .filter_map(|(node_id, _node)| {
            if voters.contains(node_id) {
                None
            } else {
                Some(*node_id)
            }
        })
        .collect()
}

pub async fn repair_membership_voters_once(
    raft: Arc<dyn RaftFacade>,
) -> anyhow::Result<BTreeSet<NodeId>> {
    repair_membership_voters_once_with_gate(raft, membership_operation_gate()).await
}

async fn repair_membership_voters_once_with_gate(
    raft: Arc<dyn RaftFacade>,
    gate: Arc<Mutex<()>>,
) -> anyhow::Result<BTreeSet<NodeId>> {
    let Ok(_membership_operation_guard) = gate.try_lock_owned() else {
        tracing::debug!(
            "raft membership guard skipped because a membership operation is in progress"
        );
        return Ok(BTreeSet::new());
    };

    let metrics = raft.metrics().borrow().clone();
    let non_voters = non_voter_membership_node_ids(&metrics);
    if non_voters.is_empty() {
        return Ok(BTreeSet::new());
    }

    if !matches!(metrics.state, openraft::ServerState::Leader) {
        anyhow::bail!(
            "stable learner repair requires leader/quorum: \
             state={:?}, current_leader={:?}, non_voters={:?}",
            metrics.state,
            metrics.current_leader,
            non_voters
        );
    }

    raft.add_voters(non_voters.clone())
        .await
        .context("promote membership nodes to voters")?;
    Ok(non_voters)
}

pub fn spawn_membership_voter_guard(
    raft: Arc<dyn RaftFacade>,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match repair_membership_voters_once(raft.clone()).await {
                Ok(promoted) if promoted.is_empty() => {}
                Ok(promoted) => {
                    tracing::info!(
                        raft_node_ids = ?promoted,
                        "raft membership guard promoted stable nodes to voters"
                    );
                }
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        "raft membership guard detected non-voter nodes but could not repair"
                    );
                }
            }
            tokio::time::sleep(interval).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use crate::raft::{
        app::BoxFuture,
        types::{ClientResponse, NodeId, NodeMeta},
    };
    use tokio::sync::{Mutex, watch};

    #[derive(Clone)]
    struct RecordingRaft {
        metrics: watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>>,
        promoted: Arc<Mutex<Vec<BTreeSet<NodeId>>>>,
    }

    impl RaftFacade for RecordingRaft {
        fn metrics(&self) -> watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>> {
            self.metrics.clone()
        }

        fn client_write(
            &self,
            _cmd: crate::state::DesiredStateCommand,
        ) -> BoxFuture<'_, anyhow::Result<ClientResponse>> {
            Box::pin(async move {
                anyhow::bail!("client_write should not be called by membership guard")
            })
        }

        fn add_learner(
            &self,
            _node_id: NodeId,
            _node: NodeMeta,
        ) -> BoxFuture<'_, anyhow::Result<()>> {
            Box::pin(async move {
                anyhow::bail!("add_learner should not be called by membership guard")
            })
        }

        fn add_voters(&self, node_ids: BTreeSet<NodeId>) -> BoxFuture<'_, anyhow::Result<()>> {
            let promoted = self.promoted.clone();
            Box::pin(async move {
                promoted.lock().await.push(node_ids);
                Ok(())
            })
        }

        fn change_membership(
            &self,
            _changes: openraft::ChangeMembers<NodeId, NodeMeta>,
            _retain: bool,
        ) -> BoxFuture<'_, anyhow::Result<()>> {
            Box::pin(async move {
                anyhow::bail!("change_membership should not be called by membership guard")
            })
        }
    }

    fn meta(name: &str) -> NodeMeta {
        NodeMeta {
            name: name.to_string(),
            api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
            raft_endpoint: xp_test_fixtures::primary_api_url().to_owned(),
        }
    }

    fn metrics_with_membership(
        state: openraft::ServerState,
        current_leader: Option<NodeId>,
        voters: BTreeSet<NodeId>,
        nodes: BTreeMap<NodeId, NodeMeta>,
    ) -> openraft::RaftMetrics<NodeId, NodeMeta> {
        let id = 1;
        let mut metrics = openraft::RaftMetrics::new_initial(id);
        metrics.current_term = 1;
        metrics.state = state;
        metrics.current_leader = current_leader;
        metrics.membership_config = Arc::new(openraft::StoredMembership::new(
            None,
            openraft::Membership::new(vec![voters], nodes),
        ));
        metrics
    }

    #[test]
    fn finds_membership_nodes_that_are_not_voters() {
        let nodes = BTreeMap::from([(1, meta("one")), (2, meta("two")), (3, meta("three"))]);
        let metrics = metrics_with_membership(
            openraft::ServerState::Leader,
            Some(1),
            BTreeSet::from([1, 3]),
            nodes,
        );

        assert_eq!(non_voter_membership_node_ids(&metrics), BTreeSet::from([2]));
    }

    #[tokio::test]
    async fn leader_repairs_legacy_non_voter_nodes() {
        let nodes = BTreeMap::from([(1, meta("one")), (2, meta("two"))]);
        let metrics = metrics_with_membership(
            openraft::ServerState::Leader,
            Some(1),
            BTreeSet::from([1]),
            nodes,
        );
        let (_tx, rx) = watch::channel(metrics);
        let promoted = Arc::new(Mutex::new(Vec::new()));
        let raft: Arc<dyn RaftFacade> = Arc::new(RecordingRaft {
            metrics: rx,
            promoted: promoted.clone(),
        });

        let repaired = repair_membership_voters_once_with_gate(raft, Arc::new(Mutex::new(())))
            .await
            .unwrap();

        assert_eq!(repaired, BTreeSet::from([2]));
        assert_eq!(*promoted.lock().await, vec![BTreeSet::from([2])]);
    }

    #[tokio::test]
    async fn follower_reports_non_voters_without_repairing() {
        let nodes = BTreeMap::from([(1, meta("one")), (2, meta("two"))]);
        let metrics = metrics_with_membership(
            openraft::ServerState::Follower,
            None,
            BTreeSet::from([1]),
            nodes,
        );
        let (_tx, rx) = watch::channel(metrics);
        let promoted = Arc::new(Mutex::new(Vec::new()));
        let raft: Arc<dyn RaftFacade> = Arc::new(RecordingRaft {
            metrics: rx,
            promoted: promoted.clone(),
        });

        let err = repair_membership_voters_once_with_gate(raft, Arc::new(Mutex::new(())))
            .await
            .unwrap_err();

        assert!(err.to_string().contains("requires leader/quorum"));
        assert!(promoted.lock().await.is_empty());
    }

    #[tokio::test]
    async fn guard_skips_when_membership_operation_gate_is_busy() {
        let nodes = BTreeMap::from([(1, meta("one")), (2, meta("two"))]);
        let metrics = metrics_with_membership(
            openraft::ServerState::Leader,
            Some(1),
            BTreeSet::from([1]),
            nodes,
        );
        let (_tx, rx) = watch::channel(metrics);
        let promoted = Arc::new(Mutex::new(Vec::new()));
        let raft: Arc<dyn RaftFacade> = Arc::new(RecordingRaft {
            metrics: rx,
            promoted: promoted.clone(),
        });
        let gate = Arc::new(Mutex::new(()));
        let _busy = gate.clone().lock_owned().await;

        let repaired = repair_membership_voters_once_with_gate(raft, gate)
            .await
            .unwrap();

        assert!(repaired.is_empty());
        assert!(promoted.lock().await.is_empty());
    }
}
