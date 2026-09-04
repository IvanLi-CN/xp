use super::*;

#[tokio::test]
async fn stale_learner_retirement_removes_only_the_confirmed_learner_and_cleans_state() {
    let temp = tempfile::tempdir().unwrap();
    let local_node_id = xp_test_fixtures::identifier_ulid_d().to_owned();
    let target_node_id = xp_test_fixtures::identifier_ulid_e().to_owned();
    let local_raft_node_id = raft_node_id_from_ulid(&local_node_id).unwrap();
    let target_raft_node_id = raft_node_id_from_ulid(&target_node_id).unwrap();
    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(StoreInit {
            data_dir: temp.path().to_path_buf(),
            bootstrap_node_id: Some(local_node_id.clone()),
            bootstrap_node_name: xp_test_fixtures::primary_node_name().to_owned(),
            bootstrap_access_host: xp_test_fixtures::primary_host().to_owned(),
            bootstrap_api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
        })
        .unwrap(),
    ));
    let target = Node {
        node_id: target_node_id.clone(),
        node_name: xp_test_fixtures::secondary_node_name().to_owned(),
        access_host: xp_test_fixtures::secondary_host().to_owned(),
        api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
        quota_limit_bytes: 0,
        quota_reset: NodeQuotaReset::default(),
    };
    let endpoint = {
        let mut store = store.lock().await;
        store.upsert_node(target.clone()).unwrap();
        store
            .create_endpoint(
                target.node_id.clone(),
                crate::domain::EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8_389,
                serde_json::json!({}),
            )
            .unwrap()
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
                    target_raft_node_id,
                    NodeMeta {
                        name: target.node_name.clone(),
                        api_base_url: target.api_base_url.clone(),
                        raft_endpoint: target.api_base_url.clone(),
                    },
                ),
            ]),
        ),
    ));
    let (metrics_tx, metrics_rx) = watch::channel(metrics);
    let retain_values = Arc::new(Mutex::new(Vec::new()));
    let raft: Arc<dyn RaftFacade> = Arc::new(RepairingRaft {
        inner: LocalRaft::new(store.clone(), metrics_rx.clone()),
        metrics: metrics_rx,
        metrics_tx,
        retain_values: retain_values.clone(),
        fail_next_membership_change: Arc::new(AtomicBool::new(false)),
    });

    let preview = preview_stale_learner_retirement(raft.clone(), store.clone(), &target_node_id)
        .await
        .unwrap();
    assert_eq!(preview.raft_node_id, target_raft_node_id);
    assert_eq!(preview.endpoint_ids, vec![endpoint.endpoint_id.clone()]);
    assert!(store.lock().await.state().membership_operations.is_empty());

    let error = begin_stale_learner_retirement(
        raft.clone(),
        store.clone(),
        &target_node_id,
        &preview.expected_membership,
        true,
        vec!["stale-endpoint".to_string()],
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("endpoint set changed"));
    assert!(store.lock().await.state().membership_operations.is_empty());

    let operation = begin_stale_learner_retirement(
        raft.clone(),
        store.clone(),
        &target_node_id,
        &preview.expected_membership,
        true,
        preview.endpoint_ids.clone(),
    )
    .await
    .unwrap();
    assert!(operation.remove_learner);
    assert_eq!(operation.kind, MembershipOperationKind::RemoveNode);

    resume_membership_operations_once(raft.clone(), store.clone())
        .await
        .unwrap();
    assert!(
        raft.metrics()
            .borrow()
            .membership_config
            .membership()
            .get_node(&target_raft_node_id)
            .is_none()
    );
    resume_membership_operations_once(raft.clone(), store.clone())
        .await
        .unwrap();
    assert!(store.lock().await.get_node(&target_node_id).is_none());
    assert!(store.lock().await.list_endpoints().is_empty());
    assert_eq!(*retain_values.lock().await, vec![false]);

    let config =
        crate::config::Cli::try_parse_from(["xp", "--data-dir", temp.path().to_str().unwrap()])
            .unwrap()
            .config;
    let cleanup = MembershipRemovalCleanup {
        local_raft_node_id,
        local_node_id,
        node_history: NodeHistoryHandle::from_config(&config),
        reconcile: ReconcileHandle::noop(),
    };
    assert!(
        finalize_remove_node_cleanup_once(raft.clone(), store.clone(), &cleanup)
            .await
            .unwrap()
    );
    assert_eq!(
        store
            .lock()
            .await
            .state()
            .membership_operations
            .get(&operation.operation_id)
            .unwrap()
            .phase,
        MembershipOperationPhase::Completed
    );
    assert!(
        crate::raft_membership_guard::audit_membership(raft, store)
            .await
            .is_clean()
    );
}
