use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn unreachable_voter_eviction_requires_internal_auth() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/admin/_internal/raft/evict-unreachable-voter")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", TEST_ADMIN_TOKEN),
                )
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "node_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn signed_preview_and_rejection_leave_unreachable_voter_state_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path().to_path_buf());
    let cluster = ClusterMetadata::init_new_cluster(
        tmp.path(),
        config.node_name.clone(),
        config.access_host.clone(),
        config.api_base_url.clone(),
    )
    .unwrap();
    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(test_store_init(&config, Some(cluster.node_id.clone())))
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
    let endpoint = {
        let mut store = store.lock().await;
        store.upsert_node(target.clone()).unwrap();
        store
            .create_endpoint(
                target.node_id.clone(),
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8_388,
                json!({}),
            )
            .unwrap()
    };
    let local_raft_node_id = raft_node_id_from_ulid(&cluster.node_id).unwrap();
    let target_raft_node_id = raft_node_id_from_ulid(&target.node_id).unwrap();
    let mut metrics = openraft::RaftMetrics::new_initial(local_raft_node_id);
    metrics.current_term = 1;
    metrics.state = openraft::ServerState::Leader;
    metrics.current_leader = Some(local_raft_node_id);
    metrics.membership_config = Arc::new(openraft::StoredMembership::new(
        None,
        openraft::Membership::new(
            vec![std::collections::BTreeSet::from([
                local_raft_node_id,
                target_raft_node_id,
            ])],
            std::collections::BTreeMap::from([
                (
                    local_raft_node_id,
                    RaftNodeMeta {
                        name: cluster.node_name.clone(),
                        api_base_url: config.api_base_url.clone(),
                        raft_endpoint: config.api_base_url.clone(),
                    },
                ),
                (
                    target_raft_node_id,
                    RaftNodeMeta {
                        name: target.node_name.clone(),
                        api_base_url: target.api_base_url.clone(),
                        raft_endpoint: target.api_base_url.clone(),
                    },
                ),
            ]),
        ),
    ));
    let (metrics_tx, metrics_rx) = watch::channel(metrics);
    let raft: Arc<dyn crate::raft::app::RaftFacade> =
        Arc::new(LocalRaft::new(store.clone(), metrics_rx));
    let app = build_app_with_cluster_store_and_raft(
        config,
        cluster.clone(),
        store.clone(),
        raft,
        ReconcileHandle::noop(),
    );
    let ca_pem = cluster.read_cluster_ca_pem(tmp.path()).unwrap();
    let ca_key_pem = cluster
        .read_cluster_ca_key_pem(tmp.path())
        .unwrap()
        .expect("cluster CA key");
    let uri: Uri = "/api/admin/_internal/raft/evict-unreachable-voter"
        .parse()
        .unwrap();
    let signed_request = |body: Vec<u8>| {
        let context = crate::internal_auth::RequestContext::now(
            crate::internal_auth::InternalRoute::MeshV2,
            &cluster.cluster_id,
            &cluster.node_id,
            &cluster.node_id,
            new_ulid_string(),
        );
        let mut headers = axum::http::HeaderMap::new();
        crate::internal_auth::sign_request_v2(
            &ca_key_pem,
            &ca_pem,
            &Method::POST,
            &uri,
            Some("application/json"),
            &body,
            &context,
            &mut headers,
        )
        .unwrap();
        let mut request = Request::builder()
            .method(Method::POST)
            .uri(uri.clone())
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap();
        request.headers_mut().extend(headers);
        request
    };

    let preview = app
        .clone()
        .oneshot(signed_request(
            serde_json::to_vec(&json!({ "node_id": target.node_id })).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(preview.status(), StatusCode::OK);
    let preview = body_json(preview).await;
    assert_eq!(preview["dry_run"], true);
    assert_eq!(preview["endpoints"][0]["endpoint_id"], endpoint.endpoint_id);
    assert!(store.lock().await.state().membership_operations.is_empty());

    let rejected = app
        .clone()
        .oneshot(signed_request(
            serde_json::to_vec(&json!({
                "node_id": target.node_id,
                "apply": true,
                "expected_membership": preview["expected_membership"],
                "delete_endpoints": true,
                "expected_endpoint_ids": [endpoint.endpoint_id, endpoint.endpoint_id],
            }))
            .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    let store_guard = store.lock().await;
    assert!(store_guard.state().membership_operations.is_empty());
    assert!(store_guard.get_node(&target.node_id).is_some());
    assert_eq!(store_guard.list_endpoints().len(), 1);
    drop(store_guard);

    let retained_node = Node {
        node_id: xp_test_fixtures::identifier_ulid_b().to_owned(),
        node_name: xp_test_fixtures::label_extra_node().to_owned(),
        access_host: xp_test_fixtures::secondary_host().to_owned(),
        api_base_url: xp_test_fixtures::url_loopback1().to_owned(),
        quota_limit_bytes: 0,
        quota_reset: NodeQuotaReset::default(),
    };
    let retained_raft_node_id = raft_node_id_from_ulid(&retained_node.node_id).unwrap();
    store
        .lock()
        .await
        .upsert_node(retained_node.clone())
        .unwrap();
    let mut barrier_metrics = metrics_tx.borrow().clone();
    let membership = barrier_metrics.membership_config.membership();
    let mut voters = membership
        .voter_ids()
        .collect::<std::collections::BTreeSet<_>>();
    voters.insert(retained_raft_node_id);
    let mut nodes = membership
        .nodes()
        .map(|(node_id, node)| (*node_id, node.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    nodes.insert(
        retained_raft_node_id,
        RaftNodeMeta {
            name: retained_node.node_name,
            api_base_url: retained_node.api_base_url.clone(),
            raft_endpoint: retained_node.api_base_url,
        },
    );
    barrier_metrics.membership_config = Arc::new(openraft::StoredMembership::new(
        *barrier_metrics.membership_config.log_id(),
        openraft::Membership::new(vec![voters], nodes),
    ));
    metrics_tx.send_replace(barrier_metrics);

    let barrier_rejected = app
        .oneshot(signed_request(
            serde_json::to_vec(&json!({
                "node_id": target.node_id,
                "apply": true,
                "expected_membership": preview["expected_membership"],
                "delete_endpoints": true,
                "expected_endpoint_ids": [endpoint.endpoint_id],
            }))
            .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(barrier_rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
    let store = store.lock().await;
    assert!(store.state().membership_operations.is_empty());
    assert!(store.get_node(&target.node_id).is_some());
    assert_eq!(store.list_endpoints().len(), 1);
}
