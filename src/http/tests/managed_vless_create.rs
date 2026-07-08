use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn create_managed_vless_derives_reality_and_accepts_canary_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "Node.Example.com.").await;

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap();

    let res = app
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": node_id,
              "kind": "vless_reality_vision_tcp",
              "port": 443,
              "canary_upstream": {
                "url": " http://127.0.0.1:8080 ",
                "mode": "h2c"
              },
              "accepted_authorities": [
                "EDGE.EXAMPLE.COM.",
                "edge.example.com:443",
                "[2001:db8::1]"
              ]
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let created = body_json(res).await;
    assert_eq!(created["meta"]["managed_default"], true);
    assert_eq!(created["meta"]["reality"]["dest"], "127.0.0.1:39043");
    assert_eq!(
        created["meta"]["reality"]["server_names"],
        json!(["Node.Example.com"])
    );
    assert_eq!(created["meta"]["reality"]["fingerprint"], "chrome");
    assert_eq!(
        created["meta"]["canary_upstream"]["url"],
        "http://127.0.0.1:8080"
    );
    assert_eq!(created["meta"]["canary_upstream"]["mode"], "h2c");
    assert_eq!(
        created["meta"]["accepted_authorities"],
        json!(["edge.example.com:443", "[2001:db8::1]:443"])
    );
}

#[tokio::test]
async fn create_managed_vless_rejects_invalid_accepted_authorities() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    set_bootstrap_node_access_host(&store, "node.example.com").await;

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap();

    let res = app
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": node_id,
              "kind": "vless_reality_vision_tcp",
              "port": 443,
              "accepted_authorities": ["https://edge.example.com:443"]
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body = body_json(res).await;
    assert_eq!(body["error"]["code"], "invalid_request");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("invalid accepted_authority")
    );
}
