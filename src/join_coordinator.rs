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
        let registered = metrics
            .membership_config
            .nodes()
            .any(|(member_id, _)| *member_id == node_id);
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
            if voters.contains(&node_id) {
                let _ = raft
                    .change_membership(
                        openraft::ChangeMembers::RemoveVoters(BTreeSet::from([node_id])),
                        true,
                    )
                    .await;
            }
            if registered {
                raft.change_membership(
                    openraft::ChangeMembers::RemoveNodes(BTreeSet::from([node_id])),
                    true,
                )
                .await?;
            }
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
            if !registered {
                let node = store
                    .lock()
                    .await
                    .get_node(&session.node_id)
                    .ok_or_else(|| anyhow::anyhow!("reserved join node is missing"))?;
                let _guard = crate::raft_membership_guard::membership_operation_gate()
                    .lock_owned()
                    .await;
                raft.add_learner(
                    node_id,
                    crate::raft::types::NodeMeta {
                        name: node.node_name.clone(),
                        api_base_url: node.api_base_url.clone(),
                        raft_endpoint: node.api_base_url.clone(),
                    },
                )
                .await?;
            }
            // A recovered reservation may already have its learner membership. Record the
            // committed state that precedes this status transition so promotion cannot use only
            // the original reservation index after a leader failover.
            session.required_log_index = session
                .required_log_index
                .max(raft.metrics().borrow().last_log_index.unwrap_or(0));
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::sync::watch;

    use super::*;
    use crate::{
        domain::{Node, NodeQuotaReset},
        join_session::JoinSession,
        raft::{
            app::{BoxFuture, LocalRaft},
            types::{ClientResponse, NodeMeta as RaftNodeMeta},
        },
        state::StoreInit,
    };

    #[derive(Clone)]
    struct RecoveringRaft {
        inner: LocalRaft,
        add_learner_calls: Arc<AtomicUsize>,
        wait_required_log_index: Arc<AtomicUsize>,
    }

    impl RaftFacade for RecoveringRaft {
        fn metrics(&self) -> watch::Receiver<openraft::RaftMetrics<u64, RaftNodeMeta>> {
            self.inner.metrics()
        }

        fn client_write(
            &self,
            cmd: DesiredStateCommand,
        ) -> BoxFuture<'_, anyhow::Result<ClientResponse>> {
            self.inner.client_write(cmd)
        }

        fn add_learner(
            &self,
            _node_id: u64,
            _node: RaftNodeMeta,
        ) -> BoxFuture<'_, anyhow::Result<()>> {
            let calls = self.add_learner_calls.clone();
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        }

        fn wait_learner_caught_up(
            &self,
            _node_id: u64,
            required_log_index: u64,
            _timeout: Duration,
        ) -> BoxFuture<'_, anyhow::Result<()>> {
            let observed = self.wait_required_log_index.clone();
            Box::pin(async move {
                observed.store(required_log_index as usize, Ordering::SeqCst);
                anyhow::bail!("learner has not started")
            })
        }

        fn add_voters(&self, _node_ids: BTreeSet<u64>) -> BoxFuture<'_, anyhow::Result<()>> {
            Box::pin(async { Ok(()) })
        }

        fn change_membership(
            &self,
            changes: openraft::ChangeMembers<u64, RaftNodeMeta>,
            retain: bool,
        ) -> BoxFuture<'_, anyhow::Result<()>> {
            self.inner.change_membership(changes, retain)
        }
    }

    #[tokio::test]
    async fn failover_re_registers_a_durable_reserved_learner() {
        let tmp = tempfile::tempdir().unwrap();
        let leader_id = xp_test_fixtures::identifier_ulid_a().to_owned();
        let learner_id = xp_test_fixtures::identifier_ulid_b().to_owned();
        let store = Arc::new(Mutex::new(
            JsonSnapshotStore::load_or_init(StoreInit {
                data_dir: tmp.path().to_owned(),
                bootstrap_node_id: Some(xp_test_fixtures::identifier_ulid_a().to_owned()),
                bootstrap_node_name: xp_test_fixtures::primary_node_name().to_owned(),
                bootstrap_access_host: xp_test_fixtures::primary_host().to_owned(),
                bootstrap_api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
            })
            .unwrap(),
        ));
        {
            let mut store = store.lock().await;
            store.state_mut().nodes.insert(
                learner_id.clone(),
                Node {
                    node_id: xp_test_fixtures::identifier_ulid_b().to_owned(),
                    node_name: xp_test_fixtures::secondary_node_name().to_owned(),
                    access_host: xp_test_fixtures::secondary_host().to_owned(),
                    api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
                    quota_limit_bytes: 0,
                    quota_reset: NodeQuotaReset::default(),
                },
            );
            store.state_mut().join_sessions.insert(
                learner_id.clone(),
                JoinSession {
                    node_id: xp_test_fixtures::identifier_ulid_b().to_owned(),
                    request_fingerprint: "fingerprint".into(),
                    signed_cert_pem: "certificate".into(),
                    token_expires_at: (Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
                    activation_deadline: (Utc::now() + chrono::Duration::minutes(10)).to_rfc3339(),
                    required_log_index: 1,
                    status: JoinSessionStatus::Reserved,
                    terminal_at: None,
                },
            );
            store.save().unwrap();
        }
        let raft_id = raft_node_id_from_ulid(&leader_id).unwrap();
        let mut metrics = openraft::RaftMetrics::new_initial(raft_id);
        metrics.state = openraft::ServerState::Leader;
        metrics.current_leader = Some(raft_id);
        metrics.membership_config = Arc::new(openraft::StoredMembership::new(
            None,
            openraft::Membership::new(
                vec![BTreeSet::from([raft_id])],
                std::collections::BTreeMap::from([(
                    raft_id,
                    RaftNodeMeta {
                        name: xp_test_fixtures::primary_node_name().to_owned(),
                        api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
                        raft_endpoint: xp_test_fixtures::primary_api_url().to_owned(),
                    },
                )]),
            ),
        ));
        let (_tx, rx) = watch::channel(metrics);
        let calls = Arc::new(AtomicUsize::new(0));
        let raft: Arc<dyn RaftFacade> = Arc::new(RecoveringRaft {
            inner: LocalRaft::new(store.clone(), rx),
            add_learner_calls: calls.clone(),
            wait_required_log_index: Arc::new(AtomicUsize::new(0)),
        });

        reconcile_once(raft, store.clone()).await.unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            store
                .lock()
                .await
                .state()
                .join_sessions
                .values()
                .next()
                .unwrap()
                .status,
            JoinSessionStatus::LearnerRegistered
        );
    }

    #[tokio::test]
    async fn failover_refreshes_required_index_for_existing_reserved_learner() {
        let tmp = tempfile::tempdir().unwrap();
        let learner_id = xp_test_fixtures::identifier_ulid_b().to_owned();
        let store = Arc::new(Mutex::new(
            JsonSnapshotStore::load_or_init(StoreInit {
                data_dir: tmp.path().to_owned(),
                bootstrap_node_id: Some(xp_test_fixtures::identifier_ulid_a().to_owned()),
                bootstrap_node_name: xp_test_fixtures::primary_node_name().to_owned(),
                bootstrap_access_host: xp_test_fixtures::primary_host().to_owned(),
                bootstrap_api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
            })
            .unwrap(),
        ));
        {
            let mut store = store.lock().await;
            store.state_mut().nodes.insert(
                learner_id.clone(),
                Node {
                    node_id: xp_test_fixtures::identifier_ulid_b().to_owned(),
                    node_name: xp_test_fixtures::secondary_node_name().to_owned(),
                    access_host: xp_test_fixtures::secondary_host().to_owned(),
                    api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
                    quota_limit_bytes: 0,
                    quota_reset: NodeQuotaReset::default(),
                },
            );
            store.state_mut().join_sessions.insert(
                learner_id,
                JoinSession {
                    node_id: xp_test_fixtures::identifier_ulid_b().to_owned(),
                    request_fingerprint: "fingerprint".into(),
                    signed_cert_pem: "certificate".into(),
                    token_expires_at: (Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
                    activation_deadline: (Utc::now() + chrono::Duration::minutes(10)).to_rfc3339(),
                    required_log_index: 1,
                    status: JoinSessionStatus::Reserved,
                    terminal_at: None,
                },
            );
            store.save().unwrap();
        }
        let leader_id = raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_a()).unwrap();
        let learner_raft_id =
            raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_b()).unwrap();
        let mut metrics = openraft::RaftMetrics::new_initial(leader_id);
        metrics.state = openraft::ServerState::Leader;
        metrics.current_leader = Some(leader_id);
        metrics.last_log_index = Some(9);
        metrics.membership_config = Arc::new(openraft::StoredMembership::new(
            None,
            openraft::Membership::new(
                vec![BTreeSet::from([leader_id])],
                std::collections::BTreeMap::from([
                    (
                        leader_id,
                        RaftNodeMeta {
                            name: xp_test_fixtures::primary_node_name().to_owned(),
                            api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
                            raft_endpoint: xp_test_fixtures::primary_api_url().to_owned(),
                        },
                    ),
                    (
                        learner_raft_id,
                        RaftNodeMeta {
                            name: xp_test_fixtures::secondary_node_name().to_owned(),
                            api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
                            raft_endpoint: xp_test_fixtures::secondary_api_url().to_owned(),
                        },
                    ),
                ]),
            ),
        ));
        let (_tx, rx) = watch::channel(metrics);
        let observed = Arc::new(AtomicUsize::new(0));
        let raft: Arc<dyn RaftFacade> = Arc::new(RecoveringRaft {
            inner: LocalRaft::new(store.clone(), rx),
            add_learner_calls: Arc::new(AtomicUsize::new(0)),
            wait_required_log_index: observed.clone(),
        });

        reconcile_once(raft, store.clone()).await.unwrap();

        assert_eq!(observed.load(Ordering::SeqCst), 9);
        assert_eq!(
            store
                .lock()
                .await
                .state()
                .join_sessions
                .values()
                .next()
                .unwrap()
                .required_log_index,
            9
        );
    }

    #[tokio::test]
    async fn expiry_without_registered_learner_still_tombstones_session() {
        let tmp = tempfile::tempdir().unwrap();
        let learner_id = xp_test_fixtures::identifier_ulid_b().to_owned();
        let store = Arc::new(Mutex::new(
            JsonSnapshotStore::load_or_init(StoreInit {
                data_dir: tmp.path().to_owned(),
                bootstrap_node_id: Some(xp_test_fixtures::identifier_ulid_a().to_owned()),
                bootstrap_node_name: xp_test_fixtures::primary_node_name().to_owned(),
                bootstrap_access_host: xp_test_fixtures::primary_host().to_owned(),
                bootstrap_api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
            })
            .unwrap(),
        ));
        {
            let mut store = store.lock().await;
            store.state_mut().nodes.insert(
                learner_id.clone(),
                Node {
                    node_id: xp_test_fixtures::identifier_ulid_b().to_owned(),
                    node_name: xp_test_fixtures::secondary_node_name().to_owned(),
                    access_host: xp_test_fixtures::secondary_host().to_owned(),
                    api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
                    quota_limit_bytes: 0,
                    quota_reset: NodeQuotaReset::default(),
                },
            );
            store.state_mut().join_sessions.insert(
                learner_id.clone(),
                JoinSession {
                    node_id: xp_test_fixtures::identifier_ulid_b().to_owned(),
                    request_fingerprint: "fingerprint".into(),
                    signed_cert_pem: "certificate".into(),
                    token_expires_at: (Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
                    activation_deadline: (Utc::now() - chrono::Duration::seconds(1)).to_rfc3339(),
                    required_log_index: 1,
                    status: JoinSessionStatus::Reserved,
                    terminal_at: None,
                },
            );
            store.save().unwrap();
        }
        let raft_id = raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_a()).unwrap();
        let mut metrics = openraft::RaftMetrics::new_initial(raft_id);
        metrics.state = openraft::ServerState::Leader;
        metrics.current_leader = Some(raft_id);
        let (_tx, rx) = watch::channel(metrics);
        let raft: Arc<dyn RaftFacade> = Arc::new(RecoveringRaft {
            inner: LocalRaft::new(store.clone(), rx),
            add_learner_calls: Arc::new(AtomicUsize::new(0)),
            wait_required_log_index: Arc::new(AtomicUsize::new(0)),
        });

        reconcile_once(raft, store.clone()).await.unwrap();

        let store = store.lock().await;
        assert_eq!(
            store.state().join_sessions.values().next().unwrap().status,
            JoinSessionStatus::Expired
        );
        assert!(
            !store
                .state()
                .nodes
                .contains_key(xp_test_fixtures::identifier_ulid_b())
        );
    }
}
