use super::*;
use pretty_assertions::assert_eq;

fn signed_preflight(tmp: &TempDir, cluster: &ClusterMetadata) -> Request<Body> {
    let uri: Uri = "/api/admin/_internal/history-repository/recovery-preflight"
        .parse()
        .unwrap();
    let context = crate::internal_auth::RequestContext::now(
        crate::internal_auth::InternalRoute::MeshV2,
        &cluster.cluster_id,
        &cluster.node_id,
        &cluster.node_id,
        new_ulid_string(),
    );
    let mut headers = axum::http::HeaderMap::new();
    crate::internal_auth::sign_request_v2(
        &cluster
            .read_cluster_ca_key_pem(tmp.path())
            .unwrap()
            .unwrap(),
        &cluster.read_cluster_ca_pem(tmp.path()).unwrap(),
        &Method::GET,
        &uri,
        None,
        b"",
        &context,
        &mut headers,
    )
    .unwrap();
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    request.headers_mut().extend(headers);
    request
}

#[tokio::test]
async fn signed_history_recovery_preflight_requires_auth_and_reports_quorum_view() {
    let tmp = tempfile::tempdir().unwrap();
    let (router, store) = app_with(&tmp, ReconcileHandle::noop());
    let cluster = ClusterMetadata::load(tmp.path()).unwrap();
    let unauthorized = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/_internal/history-repository/recovery-preflight")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    let before = serde_json::to_vec(&store.lock().await.state().repository_membership).unwrap();
    let response = router
        .oneshot(signed_preflight(&tmp, &cluster))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["node_id"], cluster.node_id);
    assert_eq!(body["version"], crate::version::VERSION);
    assert_eq!(body["quorum_verified"], true);
    assert_eq!(
        serde_json::to_vec(&store.lock().await.state().repository_membership).unwrap(),
        before
    );
}

#[tokio::test]
async fn signed_history_recovery_preflight_fails_closed_without_a_leader() {
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
    let raft = no_leader_raft(store.clone(), &cluster);
    let request = signed_preflight(&tmp, &cluster);
    let router = build_app_with_cluster_store_and_raft(
        config,
        cluster,
        store,
        raft,
        ReconcileHandle::noop(),
    );
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}
