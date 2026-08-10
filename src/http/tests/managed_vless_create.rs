use super::*;
use crate::managed_default_endpoints::{
    DefaultVlessEndpointSpec, HostManagedDefaultEndpointsOptions, ManagedDefaultEndpointsSpec,
    reconcile_host_managed_default_endpoints,
};
use crate::protocol::RealityServerNamesSource;
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

#[tokio::test]
async fn patch_managed_vless_port_survives_stale_bootstrap_reconcile() {
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
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": node_id,
              "kind": "vless_reality_vision_tcp",
              "port": 30443
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let created = body_json(res).await;
    let endpoint_id = created["endpoint_id"].as_str().unwrap().to_string();
    assert_eq!(created["meta"]["managed_default"], true);

    let res = app
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/endpoints/{endpoint_id}"),
            json!({ "port": 30445 }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let updated = body_json(res).await;
    assert_eq!(updated["port"], 30445);

    let node_endpoints = {
        let store = store.lock().await;
        store
            .list_endpoints()
            .into_iter()
            .filter(|endpoint| endpoint.node_id == node_id)
            .collect::<Vec<_>>()
    };
    let stale_bootstrap = ManagedDefaultEndpointsSpec {
        vless: Some(DefaultVlessEndpointSpec {
            port: 30443,
            reality_dest: "127.0.0.1:39043".to_string(),
            server_names: xp_test_fixtures::host_list_edge30(),
            server_names_source: RealityServerNamesSource::Manual,
            fingerprint: "chrome".to_string(),
        }),
        ss: None,
    };
    let mut writer = |command: DesiredStateCommand| {
        let store = store.clone();
        async move {
            command
                .apply(store.lock().await.state_mut())
                .map(|_| ())
                .map_err(anyhow::Error::msg)
        }
    };
    reconcile_host_managed_default_endpoints(
        tmp.path(),
        node_id,
        &node_endpoints,
        HostManagedDefaultEndpointsOptions {
            explicit: &stale_bootstrap,
            access_host: xp_test_fixtures::label_node_afixture_test(),
            vless_canary_bind: "127.0.0.1:39043".parse().unwrap(),
        },
        &mut writer,
        "test",
    )
    .await
    .unwrap();

    let endpoint = store.lock().await.get_endpoint(&endpoint_id).unwrap();
    assert_eq!(endpoint.port, 30445);
}
