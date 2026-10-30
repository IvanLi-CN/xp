use super::*;
use pretty_assertions::assert_eq;

async fn local_node_id(app: &axum::Router) -> String {
    let response = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    body_json(response).await["items"][0]["node_id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn create_vless_defaults_to_xhttp_transport() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);
    let node_id = local_node_id(&app).await;

    let response = app
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
                "node_id": node_id.clone(),
                "kind": "vless_reality_vision_tcp",
                "port": 443,
                "reality": xp_test_fixtures::endpoint_reality()
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["meta"]["transport"],
        json!("xhttp")
    );
}

#[tokio::test]
async fn xmux_default_object_clears_custom_policy_on_create_and_patch() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);
    let node_id = local_node_id(&app).await;
    let default = serde_json::to_value(crate::protocol::MihomoXmuxConfig::default()).unwrap();
    let created = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
                "node_id": node_id, "kind": "vless_reality_vision_tcp", "port": 443,
                "reality": xp_test_fixtures::endpoint_reality(), "mihomo_xmux": default
            }),
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::OK);
    let endpoint = body_json(created).await;
    assert!(endpoint["meta"].get("mihomo_xmux").is_none());
    let path = format!(
        "/api/admin/endpoints/{}",
        endpoint["endpoint_id"].as_str().unwrap()
    );
    let custom = json!({
        "max_connections": "2", "max_concurrency": "0",
        "c_max_reuse_times": "0", "h_max_reusable_secs": "0", "h_keep_alive_period": -1
    });
    let updated = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &path,
            json!({"mihomo_xmux":custom}),
        ))
        .await
        .unwrap();
    assert_eq!(updated.status(), StatusCode::OK);
    let cleared = app
        .oneshot(req_authed_json(
            "PATCH",
            &path,
            json!({"mihomo_xmux":default}),
        ))
        .await
        .unwrap();
    assert_eq!(cleared.status(), StatusCode::OK);
    assert!(
        body_json(cleared).await["meta"]
            .get("mihomo_xmux")
            .is_none()
    );
}

#[tokio::test]
async fn xmux_creation_waits_for_membership_operation_gate() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);
    let node_id = local_node_id(&app).await;
    let gate = crate::raft_membership_guard::membership_operation_gate()
        .lock_owned()
        .await;
    let request = req_authed_json(
        "POST",
        "/api/admin/endpoints",
        json!({
            "node_id": node_id,
            "kind": "vless_reality_vision_tcp",
            "port": 443,
            "reality": xp_test_fixtures::endpoint_reality(),
            "mihomo_xmux": {
                "max_connections": "2-4", "max_concurrency": "0",
                "c_max_reuse_times": "0", "h_max_reusable_secs": "0",
                "h_keep_alive_period": -1
            }
        }),
    );
    let mut pending = Box::pin(app.oneshot(request));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut pending)
            .await
            .is_err()
    );
    drop(gate);
    let response = pending.await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["meta"]["mihomo_xmux"]["max_connections"],
        "2-4"
    );
}

#[tokio::test]
async fn xmux_creation_on_follower_fails_closed() {
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
    let app = build_app_with_cluster_store_and_raft(
        config,
        cluster.clone(),
        store.clone(),
        no_leader_raft(store.clone(), &cluster),
        ReconcileHandle::noop(),
    );
    let response = app
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
                "node_id": cluster.node_id,
                "kind": "vless_reality_vision_tcp",
                "port": 443,
                "reality": xp_test_fixtures::endpoint_reality(),
                "mihomo_xmux": {
                    "max_connections": "2", "max_concurrency": "0",
                    "c_max_reuse_times": "0", "h_max_reusable_secs": "0",
                    "h_keep_alive_period": -1
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(store.lock().await.list_endpoints().is_empty());
}

#[tokio::test]
async fn xmux_policy_survives_transport_switch_and_rejects_invalid_values() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);
    let node_id = local_node_id(&app).await;
    let created = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
                "node_id": node_id,
                "kind": "vless_reality_vision_tcp",
                "port": 443,
                "reality": xp_test_fixtures::endpoint_reality()
            }),
        ))
        .await
        .unwrap();
    let endpoint = body_json(created).await;
    let path = format!(
        "/api/admin/endpoints/{}",
        endpoint["endpoint_id"].as_str().unwrap()
    );
    let policy = json!({
        "max_connections": "0", "max_concurrency": "16-32",
        "c_max_reuse_times": "2-4", "h_max_reusable_secs": "60",
        "h_keep_alive_period": 0
    });
    let response = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &path,
            json!({"mihomo_xmux":policy}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &path,
            json!({"transport":"vision_tcp"}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["meta"]["mihomo_xmux"], policy);
    let response = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &path,
            json!({"transport":"xhttp"}),
        ))
        .await
        .unwrap();
    assert_eq!(body_json(response).await["meta"]["mihomo_xmux"], policy);
    let invalid = json!({
        "max_connections": "1", "max_concurrency": "16",
        "c_max_reuse_times": "0", "h_max_reusable_secs": "0",
        "h_keep_alive_period": -1
    });
    let response = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &path,
            json!({"mihomo_xmux":invalid}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = app
        .oneshot(req_authed_json(
            "PATCH",
            &path,
            json!({"mihomo_xmux": null}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        body_json(response).await["meta"]
            .get("mihomo_xmux")
            .is_none()
    );
}

#[tokio::test]
async fn patch_vless_switches_between_xhttp_and_legacy_vision_tcp() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);
    let node_id = local_node_id(&app).await;

    let response = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
                "node_id": node_id,
                "kind": "vless_reality_vision_tcp",
                "port": 443,
                "reality": xp_test_fixtures::endpoint_reality(),
                "transport": "xhttp"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let created = body_json(response).await;
    let endpoint_id = created["endpoint_id"].as_str().unwrap();

    let response = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/endpoints/{endpoint_id}"),
            json!({ "transport": "vision_tcp" }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body_json(response).await["meta"].get("transport").is_none());

    let response = app
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/endpoints/{endpoint_id}"),
            json!({ "transport": "xhttp" }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["meta"]["transport"],
        json!("xhttp")
    );
}

#[tokio::test]
async fn patch_legacy_vless_without_transport_preserves_legacy_shape() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);
    let node_id = local_node_id(&app).await;

    let response = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
                "node_id": node_id,
                "kind": "vless_reality_vision_tcp",
                "port": 443,
                "reality": xp_test_fixtures::endpoint_reality(),
                "transport": "vision_tcp"
            }),
        ))
        .await
        .unwrap();
    let created = body_json(response).await;
    let endpoint_id = created["endpoint_id"].as_str().unwrap();
    assert!(created["meta"].get("transport").is_none());

    let response = app
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/endpoints/{endpoint_id}"),
            json!({ "port": 8443 }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let updated = body_json(response).await;
    assert_eq!(updated["port"], 8443);
    assert!(updated["meta"].get("transport").is_none());
}

#[tokio::test]
async fn patch_rejects_null_or_protocol_incompatible_transport() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);
    let node_id = local_node_id(&app).await;

    let response = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
                "node_id": node_id.clone(),
                "kind": "ss2022_2022_blake3_aes_128_gcm",
                "port": 8388
            }),
        ))
        .await
        .unwrap();
    let endpoint_id = body_json(response).await["endpoint_id"]
        .as_str()
        .unwrap()
        .to_string();

    let response = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/endpoints/{endpoint_id}"),
            json!({ "transport": "xhttp" }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        body_json(response).await["error"]["message"]
            .as_str()
            .unwrap()
            .contains("only supported for vless")
    );

    let response = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
                "node_id": node_id,
                "kind": "vless_reality_vision_tcp",
                "port": 443,
                "reality": xp_test_fixtures::endpoint_reality()
            }),
        ))
        .await
        .unwrap();
    let vless_endpoint_id = body_json(response).await["endpoint_id"]
        .as_str()
        .unwrap()
        .to_string();

    let response = app
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/endpoints/{vless_endpoint_id}"),
            json!({ "transport": null }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        body_json(response).await["error"]["message"]
            .as_str()
            .unwrap()
            .contains("transport cannot be null")
    );
}
