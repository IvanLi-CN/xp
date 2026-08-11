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
