use super::*;

#[tokio::test]
async fn expired_join_operation_is_terminalized_after_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let local_raft_node_id = raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_d()).unwrap();
    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(StoreInit {
            data_dir: temp.path().to_path_buf(),
            bootstrap_node_id: Some(xp_test_fixtures::identifier_ulid_d().to_owned()),
            bootstrap_node_name: xp_test_fixtures::primary_node_name().to_owned(),
            bootstrap_access_host: xp_test_fixtures::primary_host().to_owned(),
            bootstrap_api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
        })
        .unwrap(),
    ));

    let mut metrics = openraft::RaftMetrics::new_initial(local_raft_node_id);
    metrics.state = openraft::ServerState::Leader;
    metrics.current_leader = Some(local_raft_node_id);
    metrics.membership_config = Arc::new(openraft::StoredMembership::new(
        None,
        openraft::Membership::new(
            vec![BTreeSet::from([local_raft_node_id])],
            BTreeMap::from([(
                local_raft_node_id,
                NodeMeta {
                    name: xp_test_fixtures::primary_node_name().to_owned(),
                    api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
                    raft_endpoint: xp_test_fixtures::primary_api_url().to_owned(),
                },
            )]),
        ),
    ));
    let expected_membership = membership_revision(&metrics).unwrap();
    {
        let mut store = store.lock().await;
        store.state_mut().join_sessions.insert(
            xp_test_fixtures::identifier_ulid_e().to_owned(),
            crate::join_session::JoinSession {
                node_id: xp_test_fixtures::identifier_ulid_e().to_owned(),
                request_fingerprint: "test-fingerprint".to_string(),
                signed_cert_pem: "test-certificate".to_string(),
                token_expires_at: xp_test_fixtures::baseline_timestamp().to_owned(),
                activation_deadline: xp_test_fixtures::baseline_timestamp().to_owned(),
                required_log_index: 42,
                status: crate::join_session::JoinSessionStatus::Expired,
                terminal_at: Some(xp_test_fixtures::baseline_timestamp().to_owned()),
            },
        );
        store.state_mut().membership_operations.insert(
            "expired-join".to_string(),
            MembershipOperation {
                operation_id: "expired-join".to_string(),
                kind: MembershipOperationKind::Join,
                raft_node_id: raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_e())
                    .unwrap(),
                node_id: Some(xp_test_fixtures::identifier_ulid_e().to_owned()),
                expected_membership,
                phase: MembershipOperationPhase::LearnerRegistered,
                legacy: false,
                remove_learner: false,
                delete_endpoints: false,
                expected_endpoint_ids: Vec::new(),
                expected_endpoint_tags: Vec::new(),
                created_at: xp_test_fixtures::baseline_timestamp().to_owned(),
                next_retry_at: None,
                terminal_at: None,
                evidence: Some("test operation".to_string()),
            },
        );
    }

    let (_metrics_tx, metrics_rx) = watch::channel(metrics);
    let raft: Arc<dyn RaftFacade> = Arc::new(LocalRaft::new(store.clone(), metrics_rx));
    resume_membership_operations_once(raft, store.clone())
        .await
        .unwrap();

    let store = store.lock().await;
    let operation = store
        .state()
        .membership_operations
        .get("expired-join")
        .unwrap();
    assert_eq!(operation.phase, MembershipOperationPhase::Expired);
    assert!(operation.terminal_at.is_some());
    assert!(
        operation
            .evidence
            .as_deref()
            .is_some_and(|evidence| evidence.contains("desired state were removed"))
    );
}

#[tokio::test]
async fn expired_join_operation_blocks_when_learner_remains() {
    let temp = tempfile::tempdir().unwrap();
    let local_raft_node_id = raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_d()).unwrap();
    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(StoreInit {
            data_dir: temp.path().to_path_buf(),
            bootstrap_node_id: Some(xp_test_fixtures::identifier_ulid_d().to_owned()),
            bootstrap_node_name: xp_test_fixtures::primary_node_name().to_owned(),
            bootstrap_access_host: xp_test_fixtures::primary_host().to_owned(),
            bootstrap_api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
        })
        .unwrap(),
    ));
    let target = Node {
        node_id: xp_test_fixtures::identifier_ulid_e().to_owned(),
        node_name: xp_test_fixtures::secondary_node_name().to_owned(),
        access_host: xp_test_fixtures::secondary_host().to_owned(),
        api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
        quota_limit_bytes: 0,
        quota_reset: NodeQuotaReset::default(),
    };

    let mut metrics = openraft::RaftMetrics::new_initial(local_raft_node_id);
    metrics.state = openraft::ServerState::Leader;
    metrics.current_leader = Some(local_raft_node_id);
    metrics.membership_config = Arc::new(openraft::StoredMembership::new(
        None,
        openraft::Membership::new(
            vec![BTreeSet::from([local_raft_node_id])],
            BTreeMap::from([
                (
                    local_raft_node_id,
                    NodeMeta {
                        name: xp_test_fixtures::primary_node_name().to_owned(),
                        api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
                        raft_endpoint: xp_test_fixtures::primary_api_url().to_owned(),
                    },
                ),
                (
                    raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_e()).unwrap(),
                    NodeMeta {
                        name: xp_test_fixtures::secondary_node_name().to_owned(),
                        api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
                        raft_endpoint: xp_test_fixtures::secondary_api_url().to_owned(),
                    },
                ),
            ]),
        ),
    ));
    let expected_membership = membership_revision(&metrics).unwrap();
    {
        let mut store = store.lock().await;
        store.upsert_node(target).unwrap();
        store.state_mut().join_sessions.insert(
            xp_test_fixtures::identifier_ulid_e().to_owned(),
            crate::join_session::JoinSession {
                node_id: xp_test_fixtures::identifier_ulid_e().to_owned(),
                request_fingerprint: "test-fingerprint".to_string(),
                signed_cert_pem: "test-certificate".to_string(),
                token_expires_at: xp_test_fixtures::baseline_timestamp().to_owned(),
                activation_deadline: xp_test_fixtures::baseline_timestamp().to_owned(),
                required_log_index: 42,
                status: crate::join_session::JoinSessionStatus::Expired,
                terminal_at: Some(xp_test_fixtures::baseline_timestamp().to_owned()),
            },
        );
        store.state_mut().membership_operations.insert(
            "blocked-expired-join".to_string(),
            MembershipOperation {
                operation_id: "blocked-expired-join".to_string(),
                kind: MembershipOperationKind::Join,
                raft_node_id: raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_e())
                    .unwrap(),
                node_id: Some(xp_test_fixtures::identifier_ulid_e().to_owned()),
                expected_membership,
                phase: MembershipOperationPhase::LearnerRegistered,
                legacy: false,
                remove_learner: false,
                delete_endpoints: false,
                expected_endpoint_ids: Vec::new(),
                expected_endpoint_tags: Vec::new(),
                created_at: xp_test_fixtures::baseline_timestamp().to_owned(),
                next_retry_at: None,
                terminal_at: None,
                evidence: Some("test operation".to_string()),
            },
        );
    }

    let (_metrics_tx, metrics_rx) = watch::channel(metrics);
    let raft: Arc<dyn RaftFacade> = Arc::new(LocalRaft::new(store.clone(), metrics_rx));
    resume_membership_operations_once(raft, store.clone())
        .await
        .unwrap();

    let store = store.lock().await;
    let operation = store
        .state()
        .membership_operations
        .get("blocked-expired-join")
        .unwrap();
    assert_eq!(operation.phase, MembershipOperationPhase::Blocked);
    assert!(operation.terminal_at.is_some());
    assert!(
        operation
            .evidence
            .as_deref()
            .is_some_and(|evidence| evidence.contains("stale-learner recovery"))
    );
}
