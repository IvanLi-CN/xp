use super::*;

use pretty_assertions::assert_eq;

#[tokio::test]
async fn patch_admin_endpoint_vless_updates_meta_and_port() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

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
              "port": 443,
              "reality": {
                "dest": "example.com:443",
                "server_names": ["example.com"],
                "fingerprint": "chrome"
              }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let created = body_json(res).await;
    let endpoint_id = created["endpoint_id"].as_str().unwrap().to_string();
    let reality_keys = created["meta"]["reality_keys"].clone();
    let short_ids = created["meta"]["short_ids"].clone();
    let active_short_id = created["meta"]["active_short_id"].clone();
    assert_eq!(
        created["meta"]["mihomo_smux"],
        json!({
            "enabled": true,
            "max_connections": 4,
            "min_streams": 4,
            "only_tcp": true
        })
    );

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/endpoints/{endpoint_id}"),
            json!({ "port": 8443 }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let updated = body_json(res).await;
    assert_eq!(updated["endpoint_id"], endpoint_id);
    assert_eq!(updated["port"], 8443);
    assert_eq!(updated["meta"]["reality"]["dest"], "example.com:443");
    assert_eq!(updated["meta"]["reality"]["server_names"][0], "example.com");
    assert_eq!(updated["meta"]["reality"]["fingerprint"], "chrome");
    assert_eq!(updated["meta"]["reality_keys"], reality_keys);
    assert_eq!(updated["meta"]["short_ids"], short_ids);
    assert_eq!(updated["meta"]["active_short_id"], active_short_id);
    assert_eq!(updated["meta"].get("public_domain"), None);

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/endpoints/{endpoint_id}"),
            json!({
              "reality": {
                "dest": "edge.example.com:443",
                "server_names": ["edge.example.com"],
                "fingerprint": "firefox"
              }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let updated = body_json(res).await;
    assert_eq!(updated["endpoint_id"], endpoint_id);
    assert_eq!(updated["port"], 8443);
    assert_eq!(updated["meta"]["reality"]["dest"], "edge.example.com:443");
    assert_eq!(
        updated["meta"]["reality"]["server_names"][0],
        "edge.example.com"
    );
    assert_eq!(updated["meta"]["reality"]["fingerprint"], "firefox");
    assert_eq!(updated["meta"]["reality_keys"], reality_keys);
    assert_eq!(updated["meta"]["short_ids"], short_ids);
    assert_eq!(updated["meta"]["active_short_id"], active_short_id);
    assert_eq!(updated["meta"].get("public_domain"), None);

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/endpoints/{endpoint_id}"),
            json!({
                "mihomo_smux": {
                    "enabled": false,
                    "max_connections": 8,
                    "min_streams": 6,
                    "only_tcp": false
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let updated = body_json(res).await;
    assert_eq!(
        updated["meta"]["mihomo_smux"],
        json!({
            "enabled": false,
            "max_connections": 8,
            "min_streams": 6,
            "only_tcp": false
        })
    );

    for invalid_smux in [
        json!(null),
        json!({
            "enabled": true,
            "max_connections": 0,
            "min_streams": 4,
            "only_tcp": true
        }),
        json!({
            "enabled": true,
            "max_connections": 17,
            "min_streams": 4,
            "only_tcp": true
        }),
        json!({
            "enabled": true,
            "max_connections": 4,
            "min_streams": 0,
            "only_tcp": true
        }),
    ] {
        let res = app
            .clone()
            .oneshot(req_authed_json(
                "PATCH",
                &format!("/api/admin/endpoints/{endpoint_id}"),
                json!({ "mihomo_smux": invalid_smux }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_json(res).await["error"]["code"], "invalid_request");
    }
}

#[tokio::test]
async fn ss2022_endpoint_creation_persists_server_psk_and_password_uses_server_and_user_psk() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp);

    let res = app
        .clone()
        .oneshot(req_authed("GET", "/api/admin/nodes"))
        .await
        .unwrap();
    let nodes = body_json(res).await;
    let node_id = nodes["items"][0]["node_id"].as_str().unwrap();

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
              "node_id": node_id,
              "kind": "ss2022_2022_blake3_aes_128_gcm",
              "port": 8388
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let endpoint = body_json(res).await;
    let endpoint_id = endpoint["endpoint_id"].as_str().unwrap();

    assert_eq!(endpoint["meta"]["method"], "2022-blake3-aes-128-gcm");
    assert_eq!(
        endpoint["meta"]["mihomo_smux"],
        json!({
            "enabled": true,
            "max_connections": 4,
            "min_streams": 4,
            "only_tcp": true
        })
    );
    let server_psk_b64 = endpoint["meta"]["server_psk_b64"].as_str().unwrap();
    let server_psk = base64::engine::general_purpose::STANDARD
        .decode(server_psk_b64)
        .unwrap();
    assert_eq!(server_psk.len(), 16);

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/users",
            json!({ "display_name": "alice" }),
        ))
        .await
        .unwrap();
    let user = body_json(res).await;
    let user_id = user["user_id"].as_str().unwrap();
    let credential_epoch = user
        .get("credential_epoch")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let cluster = ClusterMetadata::load(tmp.path()).unwrap();
    let cluster_ca_key_pem = cluster
        .read_cluster_ca_key_pem(tmp.path())
        .unwrap()
        .expect("cluster ca key pem");

    let user_psk_b64 = crate::credentials::derive_ss2022_user_psk_b64(
        &cluster_ca_key_pem,
        user_id,
        credential_epoch,
    )
    .expect("derive ss2022 user_psk");
    let password = ss2022_password(server_psk_b64, &user_psk_b64);
    let (server_part, user_part) = password.split_once(':').unwrap();
    assert_eq!(server_part, server_psk_b64);
    assert_eq!(user_part, user_psk_b64);
    let user_psk = base64::engine::general_purpose::STANDARD
        .decode(user_part)
        .unwrap();
    assert_eq!(user_psk.len(), 16);

    let res = app
        .clone()
        .oneshot(req_authed_json(
            "PATCH",
            &format!("/api/admin/endpoints/{endpoint_id}"),
            json!({
                "mihomo_smux": {
                    "enabled": false,
                    "max_connections": 8,
                    "min_streams": 6,
                    "only_tcp": false
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        body_json(res).await["meta"]["mihomo_smux"],
        json!({
            "enabled": false,
            "max_connections": 8,
            "min_streams": 6,
            "only_tcp": false
        })
    );

    let res = app
        .oneshot(req_authed_json(
            "POST",
            "/api/admin/endpoints",
            json!({
                "node_id": node_id,
                "kind": "ss2022_2022_blake3_aes_128_gcm",
                "port": 8389,
                "mihomo_smux": {
                    "enabled": true,
                    "max_connections": 4,
                    "min_streams": 65,
                    "only_tcp": true
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body_json(res).await["error"]["code"], "invalid_request");
}
