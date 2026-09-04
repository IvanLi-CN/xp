use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn stale_learner_retirement_requires_internal_auth() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/admin/_internal/raft/retire-stale-learner")
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
async fn signed_stale_learner_retirement_preview_is_zero_write_and_apply_is_exact() {
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
                8_390,
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
            vec![std::collections::BTreeSet::from([local_raft_node_id])],
            std::collections::BTreeMap::from([
                (
                    local_raft_node_id,
                    RaftNodeMeta {
                        name: cluster.node_name.clone(),
                        api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
                        raft_endpoint: xp_test_fixtures::primary_api_url().to_owned(),
                    },
                ),
                (
                    target_raft_node_id,
                    RaftNodeMeta {
                        name: target.node_name.clone(),
                        api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
                        raft_endpoint: xp_test_fixtures::secondary_api_url().to_owned(),
                    },
                ),
            ]),
        ),
    ));
    let (_metrics_tx, metrics_rx) = watch::channel(metrics);
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
    let uri: Uri = "/api/admin/_internal/raft/retire-stale-learner"
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
    let store = store.lock().await;
    assert!(store.state().membership_operations.is_empty());
    assert!(store.get_node(&target.node_id).is_some());
    assert_eq!(store.list_endpoints().len(), 1);
}
