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

async fn transition_join_operation(
    raft: &Arc<dyn RaftFacade>,
    store: &Arc<Mutex<JsonSnapshotStore>>,
    node_id: u64,
    phase: crate::state::MembershipOperationPhase,
    evidence: &str,
) -> anyhow::Result<()> {
    let Some(mut operation) = store
        .lock()
        .await
        .state()
        .active_membership_operation()
        .filter(|operation| {
            operation.raft_node_id == node_id
                && operation.kind == crate::state::MembershipOperationKind::Join
        })
        .cloned()
    else {
        return Ok(());
    };
    operation.expected_membership =
        crate::raft_membership_guard::membership_revision(&raft.metrics().borrow().clone())?;
    operation.phase = phase;
    operation.evidence = Some(evidence.to_string());
    if operation.phase.is_terminal() {
        operation.terminal_at = Some(Utc::now().to_rfc3339());
    }
    write_applied(
        raft,
        DesiredStateCommand::TransitionMembershipOperation { operation },
    )
    .await?;
    Ok(())
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

/// This runs only after the caller has crossed the all-voter capability barrier and acquired the
/// lifecycle gate. It converts one replayable legacy reservation at a time; malformed legacy
/// material is preserved as terminal evidence and is never eligible for promotion.
pub async fn migrate_one_legacy_join_session(
    raft: &Arc<dyn RaftFacade>,
    store: &Arc<Mutex<JsonSnapshotStore>>,
) -> anyhow::Result<()> {
    raft.ensure_linearizable().await?;
    let metrics = raft.metrics().borrow().clone();
    if metrics.state != openraft::ServerState::Leader || metrics.current_leader != Some(metrics.id)
    {
        return Ok(());
    }
    let Some((session, node)) = ({
        let store = store.lock().await;
        if store.state().active_membership_operation().is_some() {
            None
        } else {
            store
                .state()
                .join_sessions
                .values()
                .find(|session| session.status.is_pending())
                .cloned()
                .map(|session| {
                    let node = store.get_node(&session.node_id);
                    (session, node)
                })
        }
    }) else {
        return Ok(());
    };
    let parsed_raft_node_id = raft_node_id_from_ulid(&session.node_id).ok();
    // A malformed legacy identity has no legal membership target. Keep terminal evidence without
    // ever issuing a membership action for the synthetic zero id.
    let raft_node_id = parsed_raft_node_id.unwrap_or_default();
    let membership = metrics.membership_config.membership();
    let target_is_voter = membership
        .voter_ids()
        .any(|node_id| node_id == raft_node_id);
    let target_exists = membership.get_node(&raft_node_id).is_some();
    let deadline_valid = DateTime::parse_from_rfc3339(&session.activation_deadline)
        .is_ok_and(|deadline| deadline.with_timezone(&Utc) > Utc::now());
    let replayable = parsed_raft_node_id.is_some()
        && node.as_ref().is_some_and(|node| {
            node.node_id == session.node_id
                && !session.request_fingerprint.trim().is_empty()
                && !session.signed_cert_pem.trim().is_empty()
                && deadline_valid
                && !target_is_voter
                && (target_exists || session.status == JoinSessionStatus::Reserved)
        });
    let evidence = if replayable {
        "legacy join session converted after capability barrier"
    } else {
        "legacy join session is not replayable and was blocked"
    };
    let operation = crate::state::MembershipOperation {
        operation_id: uuid::Uuid::new_v4().to_string(),
        kind: crate::state::MembershipOperationKind::Join,
        raft_node_id,
        node_id: Some(session.node_id.clone()),
        expected_membership: crate::raft_membership_guard::membership_revision(&metrics)?,
        phase: crate::state::MembershipOperationPhase::Prepared,
        legacy: true,
        delete_endpoints: false,
        expected_endpoint_ids: Vec::new(),
        expected_endpoint_tags: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        next_retry_at: None,
        terminal_at: None,
        evidence: Some(evidence.to_string()),
    };
    write_applied(
        raft,
        DesiredStateCommand::BeginMembershipOperation {
            operation: Box::new(operation.clone()),
            node: replayable.then_some(node).flatten(),
            join_session: replayable.then_some(session.clone()),
        },
    )
    .await?;
    if !replayable {
        crate::raft_membership_guard::block_membership_operation(raft, operation, evidence).await?;
    }
    Ok(())
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
        let Some(mut operation) = store
            .lock()
            .await
            .state()
            .active_membership_operation()
            .filter(|operation| {
                operation.kind == crate::state::MembershipOperationKind::Join
                    && operation.raft_node_id == node_id
                    && operation.node_id.as_deref() == Some(session.node_id.as_str())
            })
            .cloned()
        else {
            tracing::error!(
                node_id = %session.node_id,
                concat!(
                    "legacy or malformed pending join session has no durable Join operation; ",
                    "promotion is blocked"
                )
            );
            continue;
        };
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
            match operation.phase {
                crate::state::MembershipOperationPhase::LearnerRegistered => {
                    transition_join_operation(
                        &raft,
                        &store,
                        node_id,
                        crate::state::MembershipOperationPhase::VoterPromoted,
                        "join voter observed after uncertain promotion request",
                    )
                    .await?;
                }
                crate::state::MembershipOperationPhase::VoterPromoted => {}
                _ => {
                    crate::raft_membership_guard::block_membership_operation(
                        &raft,
                        operation,
                        "join voter shape is not an exact lifecycle successor",
                    )
                    .await?;
                    tracing::error!(concat!(
                        "join voter shape is not an exact lifecycle successor; ",
                        "completion is blocked"
                    ));
                    continue;
                }
            }
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
            transition_join_operation(
                &raft,
                &store,
                node_id,
                crate::state::MembershipOperationPhase::Completed,
                "join voter observed after recovery",
            )
            .await?;
            continue;
        }
        let deadline =
            DateTime::parse_from_rfc3339(&session.activation_deadline)?.with_timezone(&Utc);
        if deadline <= Utc::now() {
            if crate::raft_membership_guard::membership_revision(&metrics)?
                != operation.expected_membership
            {
                crate::raft_membership_guard::block_membership_operation(
                    &raft,
                    operation,
                    "join membership revision changed before expiry cleanup",
                )
                .await?;
                tracing::error!(
                    "join membership revision changed before expiry cleanup; cleanup is blocked"
                );
                continue;
            }
            let _guard = crate::raft_membership_guard::membership_operation_gate()
                .lock_owned()
                .await;
            if voters.contains(&node_id) {
                raft.change_membership(
                    openraft::ChangeMembers::RemoveVoters(BTreeSet::from([node_id])),
                    false,
                )
                .await?;
            } else if registered {
                raft.change_membership(
                    openraft::ChangeMembers::RemoveNodes(BTreeSet::from([node_id])),
                    false,
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
            transition_join_operation(
                &raft,
                &store,
                node_id,
                crate::state::MembershipOperationPhase::Expired,
                "join expired and its registered learner was removed",
            )
            .await?;
            continue;
        }
        if session.status == JoinSessionStatus::Reserved {
            if !registered {
                if crate::raft_membership_guard::membership_revision(&metrics)?
                    != operation.expected_membership
                {
                    crate::raft_membership_guard::block_membership_operation(
                        &raft,
                        operation,
                        "join membership revision changed before learner registration",
                    )
                    .await?;
                    tracing::error!(concat!(
                        "join membership revision changed before learner registration; ",
                        "promotion is blocked"
                    ));
                    continue;
                }
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
            let learner_observed = raft
                .metrics()
                .borrow()
                .membership_config
                .membership()
                .get_node(&node_id)
                .is_some();
            if !learner_observed {
                // `add_learner` may resolve before the local metrics watcher publishes the
                // committed membership. Keep the reservation replayable until the exact learner
                // shape is observable instead of persisting a state transition ahead of Raft.
                continue;
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
        if session.status == JoinSessionStatus::LearnerRegistered
            && operation.phase == crate::state::MembershipOperationPhase::Prepared
        {
            let learner_observed = raft
                .metrics()
                .borrow()
                .membership_config
                .membership()
                .get_node(&node_id)
                .is_some();
            if !learner_observed {
                // OpenRaft may acknowledge nonblocking learner registration before this leader's
                // metrics watcher has published it. Keep Prepared until that exact learner shape
                // is observable, so its fingerprint cannot be recorded from the old membership.
                continue;
            }
            transition_join_operation(
                &raft,
                &store,
                node_id,
                crate::state::MembershipOperationPhase::LearnerRegistered,
                "learner observed after uncertain registration request",
            )
            .await?;
            operation.phase = crate::state::MembershipOperationPhase::LearnerRegistered;
        }
        if session.status == JoinSessionStatus::LearnerRegistered
            && operation.phase != crate::state::MembershipOperationPhase::LearnerRegistered
        {
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
            if deadline <= Utc::now() {
                continue;
            }
            let current = crate::raft_membership_guard::membership_revision(
                &raft.metrics().borrow().clone(),
            )?;
            let Some(current_operation) = store
                .lock()
                .await
                .state()
                .active_membership_operation()
                .filter(|operation| {
                    operation.kind == crate::state::MembershipOperationKind::Join
                        && operation.raft_node_id == node_id
                })
                .cloned()
            else {
                anyhow::bail!("join operation disappeared before promotion")
            };
            if current != current_operation.expected_membership {
                crate::raft_membership_guard::block_membership_operation(
                    &raft,
                    current_operation,
                    "join membership revision changed before promotion",
                )
                .await?;
                tracing::error!(
                    "join membership revision changed before promotion; promotion is blocked"
                );
                continue;
            }
            raft.add_voters(BTreeSet::from([node_id])).await?;
            transition_join_operation(
                &raft,
                &store,
                node_id,
                crate::state::MembershipOperationPhase::VoterPromoted,
                "learner promoted after catch-up",
            )
            .await?;
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
            transition_join_operation(
                &raft,
                &store,
                node_id,
                crate::state::MembershipOperationPhase::Completed,
                "join completed",
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

    async fn insert_join_operation(
        store: &Arc<Mutex<JsonSnapshotStore>>,
        metrics: &openraft::RaftMetrics<u64, RaftNodeMeta>,
    ) {
        let raft_node_id = raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_b()).unwrap();
        store.lock().await.state_mut().membership_operations.insert(
            "join-operation".to_string(),
            crate::state::MembershipOperation {
                operation_id: "join-operation".to_string(),
                kind: crate::state::MembershipOperationKind::Join,
                raft_node_id,
                node_id: Some(xp_test_fixtures::identifier_ulid_b().to_owned()),
                expected_membership: crate::raft_membership_guard::membership_revision(metrics)
                    .unwrap(),
                phase: crate::state::MembershipOperationPhase::Prepared,
                legacy: false,
                delete_endpoints: false,
                expected_endpoint_ids: Vec::new(),
                expected_endpoint_tags: Vec::new(),
                created_at: xp_test_fixtures::baseline_timestamp().to_owned(),
                next_retry_at: None,
                terminal_at: None,
                evidence: Some("test join operation".to_string()),
            },
        );
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
    async fn failover_keeps_a_durable_reservation_until_metrics_observe_the_learner() {
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
        insert_join_operation(&store, &metrics).await;
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
            JoinSessionStatus::Reserved,
            "a nonblocking learner registration acknowledgement must not advance the session"
        );
        assert_eq!(
            store
                .lock()
                .await
                .state()
                .active_membership_operation()
                .unwrap()
                .phase,
            crate::state::MembershipOperationPhase::Prepared,
            concat!(
                "a nonblocking add_learner acknowledgement must not advance the operation ",
                "before metrics observe the learner"
            )
        );
    }

    #[tokio::test]
    async fn legacy_pending_session_never_registers_or_promotes_without_operation() {
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
                    activation_deadline: (Utc::now() + chrono::Duration::minutes(10)).to_rfc3339(),
                    required_log_index: 1,
                    status: JoinSessionStatus::Reserved,
                    terminal_at: None,
                },
            );
        }
        let leader_id = raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_a()).unwrap();
        let mut metrics = openraft::RaftMetrics::new_initial(leader_id);
        metrics.state = openraft::ServerState::Leader;
        metrics.current_leader = Some(leader_id);
        let (_tx, rx) = watch::channel(metrics);
        let calls = Arc::new(AtomicUsize::new(0));
        let raft: Arc<dyn RaftFacade> = Arc::new(RecoveringRaft {
            inner: LocalRaft::new(store.clone(), rx),
            add_learner_calls: calls.clone(),
            wait_required_log_index: Arc::new(AtomicUsize::new(0)),
        });

        reconcile_once(raft, store).await.unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 0);
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
        insert_join_operation(&store, &metrics).await;
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
        insert_join_operation(&store, &metrics).await;
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
