use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn ready_repository_serves_internal_queries_from_an_ordinary_cluster_peer() {
    let tmp = tempfile::tempdir().expect("temporary directory");
    let (router, store) = app_with(&tmp, ReconcileHandle::noop());
    let cluster = ClusterMetadata::load(tmp.path()).expect("cluster metadata");
    let ca_pem = cluster.read_cluster_ca_pem(tmp.path()).expect("cluster CA");
    let ca_key = cluster
        .read_cluster_ca_key_pem(tmp.path())
        .expect("cluster CA key")
        .expect("private cluster CA key");
    let peer_id = xp_test_fixtures::secondary_node_id().to_owned();
    {
        let mut store = store.lock().await;
        add_cluster_node(&mut store, &peer_id, xp_test_fixtures::secondary_node_name);
        let identity = RepositoryNodeIdentity::new(
            RepositoryNodeId::try_from(cluster.node_id.clone()).expect("repository node id"),
            Ed25519PublicKey::from_bytes([1; 32]).expect("signing key"),
            X25519PublicKey::from_bytes([2; 32]).expect("relay key"),
        )
        .expect("repository identity");
        let mut membership = RepositoryMembership::new(vec![
            RepositoryMember::new(identity, RepositoryCapacity::default())
                .expect("repository member"),
        ])
        .expect("repository membership");
        let repository_node_id =
            RepositoryNodeId::try_from(cluster.node_id.clone()).expect("repository node id");
        membership
            .mark_catch_up_complete(&repository_node_id, 1_000)
            .expect("complete catch-up");
        membership
            .mark_ready(&repository_node_id, 1_300)
            .expect("mark ready");
        store.state_mut().repository_membership = Some(membership);
    }

    let now = u64::try_from(chrono::Utc::now().timestamp()).expect("current time");
    let body = json!({
        "start_unix_seconds": now.saturating_sub(60),
        "end_unix_seconds": now,
        "page_size": 100,
    })
    .to_string();
    let uri: Uri = "/api/admin/_internal/history-repository/query"
        .parse()
        .expect("query URI");
    let context = crate::internal_auth::RequestContext::now(
        crate::internal_auth::InternalRoute::MeshV2,
        &cluster.cluster_id,
        &peer_id,
        &cluster.node_id,
        new_ulid_string(),
    );
    let mut headers = axum::http::HeaderMap::new();
    crate::internal_auth::sign_request_v2(
        &ca_key,
        &ca_pem,
        &Method::POST,
        &uri,
        Some("application/json"),
        body.as_bytes(),
        &context,
        &mut headers,
    )
    .expect("sign internal query");
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .expect("internal query request");
    request.headers_mut().extend(headers);

    let response = router
        .oneshot(request)
        .await
        .expect("internal query response");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn repository_relay_bypasses_collector_gate_only_for_ready_repository_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let (router, store) = app_with(&tmp, ReconcileHandle::noop());
    let cluster = ClusterMetadata::load(tmp.path()).unwrap();
    let ca_pem = cluster.read_cluster_ca_pem(tmp.path()).unwrap();
    let ca_key_pem = cluster
        .read_cluster_ca_key_pem(tmp.path())
        .unwrap()
        .unwrap();
    let target_repository_id = cluster.node_id.clone();
    let ready_repository_ids = vec![
        target_repository_id.clone(),
        xp_test_fixtures::identifier_ulid_a().to_owned(),
        xp_test_fixtures::identifier_ulid_b().to_owned(),
        xp_test_fixtures::identifier_ulid_c().to_owned(),
    ];
    let source_repository_id = ready_repository_ids
        .iter()
        .find(|candidate| {
            **candidate != target_repository_id
                && rendezvous_collectors(candidate, &ready_repository_ids).is_ok_and(|assignment| {
                    assignment.primary() != target_repository_id
                        && assignment.standby() != Some(target_repository_id.as_str())
                })
        })
        .expect("a ready source assigned away from the target")
        .to_owned();
    let relay_repository_id = ready_repository_ids
        .iter()
        .find(|candidate| {
            **candidate != target_repository_id && **candidate != source_repository_id
        })
        .expect("independent relay repository")
        .to_owned();
    let ordinary_source_id = (0..32)
        .map(|index| format!("ordinary-history-source-{index}"))
        .find(|candidate| {
            rendezvous_collectors(candidate, &ready_repository_ids).is_ok_and(|assignment| {
                assignment.primary() != target_repository_id
                    && assignment.standby() != Some(target_repository_id.as_str())
            })
        })
        .expect("an ordinary source assigned away from the target");

    {
        let mut store = store.lock().await;
        let template = store
            .state()
            .nodes
            .get(&target_repository_id)
            .cloned()
            .expect("local test node");
        for node_id in ready_repository_ids
            .iter()
            .chain(std::iter::once(&ordinary_source_id))
        {
            if node_id == &target_repository_id {
                continue;
            }
            let mut node = template.clone();
            node.node_id = node_id.clone();
            node.node_name = format!("history-{node_id}");
            store.state_mut().nodes.insert(node_id.clone(), node);
        }
        let mut membership = RepositoryMembership::new(
            ready_repository_ids
                .iter()
                .enumerate()
                .map(|(index, node_id)| {
                    let marker = u8::try_from(index)
                        .expect("small fixture index")
                        .saturating_add(1);
                    let identity = RepositoryNodeIdentity::new(
                        RepositoryNodeId::try_from(node_id.clone()).expect("repository node id"),
                        Ed25519PublicKey::from_bytes([marker; 32]).expect("signing key"),
                        X25519PublicKey::from_bytes([marker.saturating_add(1); 32])
                            .expect("relay key"),
                    )
                    .expect("repository identity");
                    RepositoryMember::new(identity, RepositoryCapacity::default())
                        .expect("repository member")
                })
                .collect(),
        )
        .expect("repository membership");
        let repository_node_ids = membership
            .members()
            .iter()
            .map(|member| member.node_id().clone())
            .collect::<Vec<_>>();
        for repository_node_id in &repository_node_ids {
            membership
                .mark_catch_up_complete(repository_node_id, 1_000)
                .expect("complete catch-up");
            membership
                .mark_ready(repository_node_id, 1_300)
                .expect("mark ready");
        }
        store.state_mut().repository_membership = Some(membership);
    }

    let relay_keypair = |node_id: &str| {
        let mut hasher = Sha256::new();
        hasher.update(b"xp-history-repository-relay-key-v1\0");
        hasher.update(cluster.cluster_id.as_bytes());
        hasher.update([0]);
        hasher.update(node_id.as_bytes());
        hasher.update([0]);
        hasher.update(ca_key_pem.as_bytes());
        RelayKeypair::from_private_key(hasher.finalize().into())
    };
    let relay_payload = zstd::stream::encode_all(
        std::io::Cursor::new(serde_json::to_vec(&json!({ "segments": [], "gaps": [] })).unwrap()),
        1,
    )
    .expect("relay payload");
    let uri: Uri = "/api/admin/_internal/history-repository/relay-deliver"
        .parse()
        .unwrap();
    let signed_request = |source_repository_id: &str| {
        let frame = RelayFrame::seal(
            relay_keypair(source_repository_id),
            relay_keypair(&target_repository_id).public_key(),
            [17; 12],
            &relay_payload,
            target_repository_id.as_bytes(),
        )
        .expect("sealed relay frame");
        let body = json!({
            "target_repository_id": target_repository_id,
            "source_repository_id": source_repository_id,
            "relay_repository_id": relay_repository_id,
            "frame": frame,
        })
        .to_string();
        let context = crate::internal_auth::RequestContext::now(
            crate::internal_auth::InternalRoute::MeshV2,
            &cluster.cluster_id,
            &relay_repository_id,
            &target_repository_id,
            new_ulid_string(),
        );
        let mut headers = axum::http::HeaderMap::new();
        crate::internal_auth::sign_request_v2(
            &ca_key_pem,
            &ca_pem,
            &Method::POST,
            &uri,
            Some("application/json"),
            body.as_bytes(),
            &context,
            &mut headers,
        )
        .expect("signed relay request");
        let mut request = Request::builder()
            .method(Method::POST)
            .uri(uri.clone())
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .expect("relay request");
        request.headers_mut().extend(headers);
        request
    };

    let ready_response = router
        .clone()
        .oneshot(signed_request(&source_repository_id))
        .await
        .expect("ready repository relay response");
    assert_eq!(ready_response.status(), StatusCode::OK);

    let ordinary_response = router
        .oneshot(signed_request(&ordinary_source_id))
        .await
        .expect("ordinary source relay response");
    assert_eq!(ordinary_response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn initial_backfill_exports_historical_data_from_an_authenticated_peer() {
    let tmp = tempfile::tempdir().expect("temporary directory");
    let config = test_config(tmp.path().to_path_buf());
    let mut cluster = ClusterMetadata::init_new_cluster(
        tmp.path(),
        config.node_name.clone(),
        config.access_host.clone(),
        config.api_base_url.clone(),
    )
    .expect("cluster");
    cluster.node_id = xp_test_fixtures::identifier_ulid_d().to_owned();
    cluster.save(tmp.path()).expect("save cluster");
    let peer_id = xp_test_fixtures::secondary_node_id().to_owned();
    let history = crate::node_history::NodeHistoryHandle::from_config(&config);
    history
        .replace_node_snapshot(
            chrono::Utc::now(),
            &cluster.node_id,
            crate::node_history::NodeHistorySnapshot {
                node_id: xp_test_fixtures::identifier_ulid_d().to_owned(),
                last_synced_at: Some("2026-08-14T00:00:00Z".to_owned()),
                last_sync_error: None,
                daily_traffic: vec![crate::node_history::NodeHistoryDailyTraffic {
                    date: "2026-08-14".to_owned(),
                    uplink_bytes: 42,
                    downlink_bytes: 24,
                    updated_at: "2026-08-14T00:00:00Z".to_owned(),
                }],
                daily_component_status: Vec::new(),
                component_status_events: Vec::new(),
                traffic: None,
                user_traffic_users: Vec::new(),
            },
        )
        .await;
    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(test_store_init(&config, Some(cluster.node_id.clone())))
            .expect("store"),
    ));
    {
        let mut store = store.lock().await;
        add_cluster_node(&mut store, &peer_id, xp_test_fixtures::secondary_node_name);
    }
    let router = build_app_with_cluster_store_and_raft(
        config.clone(),
        cluster.clone(),
        store.clone(),
        leader_raft(store, &cluster),
        ReconcileHandle::noop(),
    );
    let ca_pem = cluster.read_cluster_ca_pem(tmp.path()).expect("cluster CA");
    let ca_key = cluster
        .read_cluster_ca_key_pem(tmp.path())
        .expect("cluster CA key")
        .expect("private cluster CA key");
    let uri: Uri = "/api/admin/_internal/history-repository/initial-backfill"
        .parse()
        .expect("backfill URI");
    let context = crate::internal_auth::RequestContext::now(
        crate::internal_auth::InternalRoute::MeshV2,
        &cluster.cluster_id,
        &peer_id,
        &cluster.node_id,
        new_ulid_string(),
    );
    let mut headers = axum::http::HeaderMap::new();
    crate::internal_auth::sign_request_v2(
        &ca_key,
        &ca_pem,
        &Method::GET,
        &uri,
        None,
        b"",
        &context,
        &mut headers,
    )
    .expect("sign peer history request");
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .expect("peer history request");
    request.headers_mut().extend(headers);

    let response = router
        .oneshot(request)
        .await
        .expect("peer history response");
    assert_eq!(response.status(), StatusCode::OK);
    let page = body_json(response).await;
    assert!(
        page["records"]
            .as_array()
            .expect("backfill records")
            .iter()
            .any(|record| record["subject_node_id"] == cluster.node_id)
    );
}
