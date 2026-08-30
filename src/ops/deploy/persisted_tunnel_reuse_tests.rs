use super::*;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn build_plan_reuses_matching_persisted_tunnel_without_conflict() {
    let _lock = crate::ops::util::ENV_LOCK.lock().unwrap();
    let server = MockServer::start().await;
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let xp_bin = tmp.path().join("xp");
    fs::write(&xp_bin, b"dummy").unwrap();
    fs::create_dir_all(paths.etc_xp_ops_cloudflare_dir()).unwrap();
    fs::create_dir_all(paths.etc_cloudflared_dir()).unwrap();
    fs::write(
        paths.etc_xp_ops_cloudflare_settings(),
        serde_json::json!({
            "account_id": "account",
            "zone_id": "zone",
            "hostname": xp_test_fixtures::host_fixture553(),
            "tunnel_id": "existing",
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        paths.etc_cloudflared_dir().join("existing.json"),
        r#"{"TunnelID":"existing"}"#,
    )
    .unwrap();

    Mock::given(method("GET"))
        .and(path("/client/v4/zones/zone"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "errors": [],
            "result": { "id": "zone", "name": "example.test", "account": { "id": "account" } }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/client/v4/accounts/account/cfd_tunnel"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "errors": [],
            "result": [{ "id": "existing", "name": "xp-node" }]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/client/v4/zones/zone/dns_records"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "errors": [],
            "result": []
        })))
        .expect(1)
        .mount(&server)
        .await;

    let args = DeployArgs {
        xp_bin: Some(xp_bin),
        node_name: xp_test_fixtures::label_node1_variant2().to_owned(),
        access_host: xp_test_fixtures::host_fixture552().to_owned(),
        cloudflare_toggle: crate::ops::cli::CloudflareToggle {
            cloudflare: true,
            no_cloudflare: false,
        },
        ddns_toggle: crate::ops::cli::DdnsToggle {
            ddns: false,
            no_ddns: true,
        },
        ip_geo_enabled: false,
        account_id: Some("account".to_string()),
        zone_id: Some("zone".to_string()),
        hostname: Some(xp_test_fixtures::host_fixture553().to_owned()),
        tunnel_name: Some("xp-node".to_string()),
        origin_url: Some("http://127.0.0.1:62416".to_string()),
        migrate_existing_tunnel: false,
        ddns_zone_id: None,
        vless_canary_acme_contact_email: None,
        default_vless_port: None,
        default_vless_server_names: None,
        default_vless_fingerprint: None,
        default_ss_port: None,
        join_token: None,
        join_token_stdin: false,
        join_token_stdin_value: None,
        cloudflare_token: Some("token".to_string()),
        cloudflare_token_stdin: false,
        cloudflare_token_stdin_value: None,
        api_base_url: xp_test_fixtures::none(),
        xray_version: "latest".to_string(),
        enable_services_toggle: crate::ops::cli::EnableServicesToggle {
            enable_services: false,
            no_enable_services: true,
        },
        yes: true,
        overwrite_existing: false,
        non_interactive: true,
        dry_run: true,
    };

    unsafe { std::env::set_var("CLOUDFLARE_API_BASE_URL", server.uri()) };
    let plan = build_plan(&paths, &args).await.unwrap();
    unsafe { std::env::remove_var("CLOUDFLARE_API_BASE_URL") };

    let cloudflare = plan.cloudflare.expect("cloudflare plan");
    assert!(
        cloudflare.tunnel_conflict.is_none(),
        "matching persisted Tunnel must not enter automatic conflict resolution"
    );
    assert_eq!(
        cloudflare
            .tunnel_override
            .as_ref()
            .map(|tunnel| tunnel.id.as_str()),
        Some("existing")
    );
}
