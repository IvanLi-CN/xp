use super::*;

#[tokio::test]
async fn unreachable_voter_eviction_requires_a_previewed_mapped_voter_and_endpoint_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let local_node_id = xp_test_fixtures::identifier_ulid_d().to_owned();
    let target_node_id = xp_test_fixtures::identifier_ulid_e().to_owned();
    let local_raft_node_id = raft_node_id_from_ulid(&local_node_id).unwrap();
    let target_raft_node_id = raft_node_id_from_ulid(&target_node_id).unwrap();
    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(StoreInit {
            data_dir: temp.path().to_path_buf(),
            bootstrap_node_id: Some(local_node_id),
            bootstrap_node_name: xp_test_fixtures::primary_node_name().to_owned(),
            bootstrap_access_host: xp_test_fixtures::primary_host().to_owned(),
            bootstrap_api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
        })
        .unwrap(),
    ));
    store
        .lock()
        .await
        .upsert_node(Node {
            node_id: target_node_id.clone(),
            node_name: xp_test_fixtures::secondary_node_name().to_owned(),
            access_host: xp_test_fixtures::secondary_host().to_owned(),
            api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        })
        .unwrap();
    let endpoint = store
        .lock()
        .await
        .create_endpoint(
            target_node_id.clone(),
            crate::domain::EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            8_388,
            serde_json::json!({}),
        )
        .unwrap();
    // Two healthy spare voters keep the fixture above the post-eviction voter floor.
    let spares = [
        xp_test_fixtures::identifier_ulid_a(),
        xp_test_fixtures::identifier_ulid_b(),
    ];
    let mut spare_voters = Vec::new();
    for spare_id in spares {
        store
            .lock()
            .await
            .upsert_node(Node {
                node_id: spare_id.to_owned(),
                node_name: format!("spare-{}", &spare_id[spare_id.len() - 4..]),
                access_host: xp_test_fixtures::secondary_host().to_owned(),
                api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
                quota_limit_bytes: 0,
                quota_reset: NodeQuotaReset::default(),
            })
            .unwrap();
        spare_voters.push(raft_node_id_from_ulid(spare_id).unwrap());
    }

    let mut metrics = openraft::RaftMetrics::new_initial(local_raft_node_id);
    metrics.state = openraft::ServerState::Leader;
    metrics.current_leader = Some(local_raft_node_id);
    let mut voters = BTreeSet::from([local_raft_node_id, target_raft_node_id]);
    voters.extend(spare_voters.iter().copied());
    let mut nodes = BTreeMap::from([
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
                name: xp_test_fixtures::secondary_node_name().to_owned(),
                api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
                raft_endpoint: xp_test_fixtures::secondary_api_url().to_owned(),
            },
        ),
    ]);
    for spare in &spare_voters {
        nodes.insert(
            *spare,
            NodeMeta {
                name: format!("spare-{spare}"),
                api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
                raft_endpoint: xp_test_fixtures::secondary_api_url().to_owned(),
            },
        );
    }
    metrics.membership_config = Arc::new(openraft::StoredMembership::new(
        None,
        openraft::Membership::new(vec![voters], nodes),
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

    let preview =
        preview_unreachable_voter_eviction(raft.clone(), store.clone(), &target_node_id, &[])
            .await
            .unwrap();
    assert_eq!(preview.raft_node_id, target_raft_node_id);
    assert_eq!(preview.endpoint_ids, vec![endpoint.endpoint_id.clone()]);
    assert!(store.lock().await.state().membership_operations.is_empty());

    let error = begin_unreachable_voter_eviction(
        raft.clone(),
        store.clone(),
        &target_node_id,
        &preview.expected_membership,
        true,
        vec!["stale-endpoint".to_string()],
        &[],
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("endpoint set changed"));
    assert!(store.lock().await.state().membership_operations.is_empty());

    let operation = begin_unreachable_voter_eviction(
        raft.clone(),
        store.clone(),
        &target_node_id,
        &preview.expected_membership,
        true,
        preview.endpoint_ids.clone(),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(operation.kind, MembershipOperationKind::RemoveNode);
    assert_eq!(operation.node_id.as_deref(), Some(target_node_id.as_str()));
    assert_eq!(operation.expected_endpoint_tags, preview.endpoint_tags);

    resume_membership_operations_once(raft.clone(), store.clone())
        .await
        .unwrap();
    resume_membership_operations_once(raft.clone(), store.clone())
        .await
        .unwrap();
    assert!(store.lock().await.get_node(&target_node_id).is_none());
    assert!(store.lock().await.list_endpoints().is_empty());
    assert!(
        raft.metrics()
            .borrow()
            .membership_config
            .membership()
            .get_node(&target_raft_node_id)
            .is_none()
    );
    assert_eq!(*retain_values.lock().await, vec![false]);
}

#[tokio::test]
async fn unreachable_voter_eviction_rejects_a_confirmed_set_below_the_voter_floor() {
    let temp = tempfile::tempdir().unwrap();
    let local_node_id = xp_test_fixtures::identifier_ulid_d().to_owned();
    let target_node_id = xp_test_fixtures::identifier_ulid_e().to_owned();
    let local_raft_node_id = raft_node_id_from_ulid(&local_node_id).unwrap();
    let target_raft_node_id = raft_node_id_from_ulid(&target_node_id).unwrap();
    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(StoreInit {
            data_dir: temp.path().to_path_buf(),
            bootstrap_node_id: Some(local_node_id),
            bootstrap_node_name: xp_test_fixtures::primary_node_name().to_owned(),
            bootstrap_access_host: xp_test_fixtures::primary_host().to_owned(),
            bootstrap_api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
        })
        .unwrap(),
    ));
    store
        .lock()
        .await
        .upsert_node(Node {
            node_id: target_node_id.clone(),
            node_name: xp_test_fixtures::secondary_node_name().to_owned(),
            access_host: xp_test_fixtures::secondary_host().to_owned(),
            api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        })
        .unwrap();

    let mut metrics = openraft::RaftMetrics::new_initial(local_raft_node_id);
    metrics.state = openraft::ServerState::Leader;
    metrics.current_leader = Some(local_raft_node_id);
    metrics.membership_config = Arc::new(openraft::StoredMembership::new(
        None,
        openraft::Membership::new(
            vec![BTreeSet::from([local_raft_node_id, target_raft_node_id])],
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
                        name: xp_test_fixtures::secondary_node_name().to_owned(),
                        api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
                        raft_endpoint: xp_test_fixtures::secondary_api_url().to_owned(),
                    },
                ),
            ]),
        ),
    ));
    let (_metrics_tx, metrics_rx) = watch::channel(metrics);
    let raft: Arc<dyn RaftFacade> = Arc::new(LocalRaft::new(store.clone(), metrics_rx));

    let error = preview_unreachable_voter_eviction(raft, store, &target_node_id, &[])
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("at least 3 are required"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn unreachable_voter_eviction_rejects_the_target_in_the_confirmed_set() {
    let temp = tempfile::tempdir().unwrap();
    let local_node_id = xp_test_fixtures::identifier_ulid_d().to_owned();
    let target_node_id = xp_test_fixtures::identifier_ulid_e().to_owned();
    let local_raft_node_id = raft_node_id_from_ulid(&local_node_id).unwrap();
    let target_raft_node_id = raft_node_id_from_ulid(&target_node_id).unwrap();
    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(StoreInit {
            data_dir: temp.path().to_path_buf(),
            bootstrap_node_id: Some(local_node_id),
            bootstrap_node_name: xp_test_fixtures::primary_node_name().to_owned(),
            bootstrap_access_host: xp_test_fixtures::primary_host().to_owned(),
            bootstrap_api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
        })
        .unwrap(),
    ));
    store
        .lock()
        .await
        .upsert_node(Node {
            node_id: target_node_id.clone(),
            node_name: xp_test_fixtures::secondary_node_name().to_owned(),
            access_host: xp_test_fixtures::secondary_host().to_owned(),
            api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        })
        .unwrap();

    let mut voters = BTreeSet::from([local_raft_node_id, target_raft_node_id]);
    let mut nodes = BTreeMap::from([
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
                name: xp_test_fixtures::secondary_node_name().to_owned(),
                api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
                raft_endpoint: xp_test_fixtures::secondary_api_url().to_owned(),
            },
        ),
    ]);
    for spare_id in [
        xp_test_fixtures::identifier_ulid_a(),
        xp_test_fixtures::identifier_ulid_b(),
    ] {
        let spare = raft_node_id_from_ulid(spare_id).unwrap();
        store
            .lock()
            .await
            .upsert_node(Node {
                node_id: spare_id.to_owned(),
                node_name: format!("spare-{spare}"),
                access_host: xp_test_fixtures::secondary_host().to_owned(),
                api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
                quota_limit_bytes: 0,
                quota_reset: NodeQuotaReset::default(),
            })
            .unwrap();
        voters.insert(spare);
        nodes.insert(
            spare,
            NodeMeta {
                name: format!("spare-{spare}"),
                api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
                raft_endpoint: xp_test_fixtures::secondary_api_url().to_owned(),
            },
        );
    }

    let mut metrics = openraft::RaftMetrics::new_initial(local_raft_node_id);
    metrics.state = openraft::ServerState::Leader;
    metrics.current_leader = Some(local_raft_node_id);
    metrics.membership_config = Arc::new(openraft::StoredMembership::new(
        None,
        openraft::Membership::new(vec![voters], nodes),
    ));
    let (_metrics_tx, metrics_rx) = watch::channel(metrics);
    let raft: Arc<dyn RaftFacade> = Arc::new(LocalRaft::new(store.clone(), metrics_rx));

    let error =
        preview_unreachable_voter_eviction(raft, store, &target_node_id, &[target_raft_node_id])
            .await
            .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("must exclude the eviction target"),
        "unexpected error: {error}"
    );
}
