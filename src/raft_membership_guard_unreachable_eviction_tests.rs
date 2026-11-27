use super::*;
use crate::raft_membership_guard::EvictionConfirmation;

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
            bootstrap_node_id: Some(xp_test_fixtures::identifier_ulid_d().to_owned()),
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
            node_id: xp_test_fixtures::identifier_ulid_e().to_owned(),
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
    let first_spare = raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_a()).unwrap();
    let second_spare = raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_b()).unwrap();
    let spare_voters = [first_spare, second_spare];
    store
        .lock()
        .await
        .upsert_node(Node {
            node_id: xp_test_fixtures::identifier_ulid_a().to_owned(),
            node_name: xp_test_fixtures::label_extra_node().to_owned(),
            access_host: xp_test_fixtures::secondary_host().to_owned(),
            api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        })
        .unwrap();
    store
        .lock()
        .await
        .upsert_node(Node {
            node_id: xp_test_fixtures::identifier_ulid_b().to_owned(),
            node_name: xp_test_fixtures::label_node1().to_owned(),
            access_host: xp_test_fixtures::secondary_host().to_owned(),
            api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        })
        .unwrap();

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
                name: xp_test_fixtures::label_extra_node().to_owned(),
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

    let preview = preview_unreachable_voter_eviction(
        raft.clone(),
        store.clone(),
        &target_node_id,
        &[],
        false,
    )
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
        EvictionConfirmation {
            delete_endpoints: true,
            expected_endpoint_ids: vec!["stale-endpoint".to_string()],
            confirmed_unreachable: &[],
            confirmed_absent: false,
        },
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
        EvictionConfirmation {
            delete_endpoints: true,
            expected_endpoint_ids: preview.endpoint_ids.clone(),
            confirmed_unreachable: &[],
            confirmed_absent: false,
        },
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
            bootstrap_node_id: Some(xp_test_fixtures::identifier_ulid_d().to_owned()),
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
            node_id: xp_test_fixtures::identifier_ulid_e().to_owned(),
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

    let error = preview_unreachable_voter_eviction(raft, store, &target_node_id, &[], false)
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
            bootstrap_node_id: Some(xp_test_fixtures::identifier_ulid_d().to_owned()),
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
            node_id: xp_test_fixtures::identifier_ulid_e().to_owned(),
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
    let identifier_ulid_a_spare =
        raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_a()).unwrap();
    store
        .lock()
        .await
        .upsert_node(Node {
            node_id: xp_test_fixtures::identifier_ulid_a().to_owned(),
            node_name: xp_test_fixtures::label_extra_node().to_owned(),
            access_host: xp_test_fixtures::secondary_host().to_owned(),
            api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        })
        .unwrap();
    voters.insert(identifier_ulid_a_spare);
    nodes.insert(
        identifier_ulid_a_spare,
        NodeMeta {
            name: xp_test_fixtures::label_extra_node().to_owned(),
            api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
            raft_endpoint: xp_test_fixtures::secondary_api_url().to_owned(),
        },
    );
    let identifier_ulid_b_spare =
        raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_b()).unwrap();
    store
        .lock()
        .await
        .upsert_node(Node {
            node_id: xp_test_fixtures::identifier_ulid_b().to_owned(),
            node_name: xp_test_fixtures::label_node1().to_owned(),
            access_host: xp_test_fixtures::secondary_host().to_owned(),
            api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        })
        .unwrap();
    voters.insert(identifier_ulid_b_spare);
    nodes.insert(
        identifier_ulid_b_spare,
        NodeMeta {
            name: xp_test_fixtures::label_node1().to_owned(),
            api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
            raft_endpoint: xp_test_fixtures::secondary_api_url().to_owned(),
        },
    );

    let mut metrics = openraft::RaftMetrics::new_initial(local_raft_node_id);
    metrics.state = openraft::ServerState::Leader;
    metrics.current_leader = Some(local_raft_node_id);
    metrics.membership_config = Arc::new(openraft::StoredMembership::new(
        None,
        openraft::Membership::new(vec![voters], nodes),
    ));
    let (_metrics_tx, metrics_rx) = watch::channel(metrics);
    let raft: Arc<dyn RaftFacade> = Arc::new(LocalRaft::new(store.clone(), metrics_rx));

    let error = preview_unreachable_voter_eviction(
        raft,
        store,
        &target_node_id,
        &[target_raft_node_id],
        false,
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("must exclude the eviction target"),
        "unexpected error: {error}"
    );
}

/// Removing several absent DesiredState Nodes must not deadlock on its own siblings, but the
/// absence mode must not excuse any other incident shape either.
async fn absent_pair_fixture(
    include_orphan_voter: bool,
) -> (
    Arc<dyn RaftFacade>,
    Arc<Mutex<JsonSnapshotStore>>,
    String,
    NodeId,
    NodeId,
    tempfile::TempDir,
) {
    let temp = tempfile::tempdir().unwrap();
    let local_raft_node_id = raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_d()).unwrap();
    let absent_a = xp_test_fixtures::identifier_ulid_e().to_owned();
    let absent_b = xp_test_fixtures::identifier_ulid_a().to_owned();
    let absent_a_raft = raft_node_id_from_ulid(&absent_a).unwrap();
    let absent_b_raft = raft_node_id_from_ulid(&absent_b).unwrap();

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
    for node in [
        Node {
            node_id: xp_test_fixtures::identifier_ulid_e().to_owned(),
            node_name: xp_test_fixtures::label_extra_node().to_owned(),
            access_host: xp_test_fixtures::secondary_host().to_owned(),
            api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
        Node {
            node_id: xp_test_fixtures::identifier_ulid_a().to_owned(),
            node_name: xp_test_fixtures::label_node1().to_owned(),
            access_host: xp_test_fixtures::secondary_host().to_owned(),
            api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        },
    ] {
        store.lock().await.upsert_node(node).unwrap();
    }

    let mut voters = BTreeSet::from([local_raft_node_id]);
    let mut nodes = BTreeMap::from([(
        local_raft_node_id,
        NodeMeta {
            name: xp_test_fixtures::primary_node_name().to_owned(),
            api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
            raft_endpoint: xp_test_fixtures::primary_api_url().to_owned(),
        },
    )]);
    if include_orphan_voter {
        // A voter with no DesiredState node: an incident no absence mode may tolerate.
        let orphan_raft = raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_b()).unwrap();
        voters.insert(orphan_raft);
        nodes.insert(
            orphan_raft,
            NodeMeta {
                name: xp_test_fixtures::label_node1().to_owned(),
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
    std::mem::forget(_metrics_tx);
    let raft: Arc<dyn RaftFacade> = Arc::new(LocalRaft::new(store.clone(), metrics_rx));
    (raft, store, absent_a, absent_a_raft, absent_b_raft, temp)
}

#[tokio::test]
async fn absent_node_removal_tolerates_unrelated_absent_peers() {
    let (raft, store, _a, absent_a_raft, _b, _temp) = absent_pair_fixture(false).await;

    crate::raft_membership_guard::require_clean_membership_except_absent(
        raft,
        store,
        &[absent_a_raft],
    )
    .await
    .expect("an unrelated absent peer must not block this removal");
}

#[tokio::test]
async fn absent_node_removal_still_blocks_other_incidents() {
    let (raft, store, _a, absent_a_raft, _b, _temp) = absent_pair_fixture(true).await;

    let error = crate::raft_membership_guard::require_clean_membership_except_absent(
        raft,
        store,
        &[absent_a_raft],
    )
    .await
    .expect_err("orphan voters remain a blocking incident");
    assert!(
        error.to_string().contains("orphan_voters"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn absent_node_removal_rejects_a_target_that_is_not_absent() {
    let (raft, store, _a, _absent_a_raft, _b, _temp) = absent_pair_fixture(false).await;
    // The surviving leader is a voter, so it is not part of the absent set. Accepting it here
    // would mean the mode clears absence evidence for a node that never needed removing.
    let leader_raft = raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_d()).unwrap();

    let error = crate::raft_membership_guard::require_clean_membership_except_absent(
        raft,
        store,
        &[leader_raft],
    )
    .await
    .expect_err("a voter is not an absent desired state member");
    assert!(
        error
            .to_string()
            .contains("not an absent desired state member"),
        "unexpected error: {error}"
    );
}
