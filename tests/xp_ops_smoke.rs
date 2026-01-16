use predicates::prelude::*;
use std::fs;

#[test]
fn cloudflare_token_set_dry_run_redacts_token() {
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("xp-ops");
    cmd.env("CLOUDFLARE_API_TOKEN", "supersecret");
    cmd.args([
        "cloudflare",
        "token",
        "set",
        "--from-env",
        "CLOUDFLARE_API_TOKEN",
        "--dry-run",
    ]);
    cmd.assert()
        .success()
        .stderr(predicate::str::contains("would write token"))
        .stderr(predicate::str::contains("supersecret").not());
}

#[test]
fn cloudflare_provision_missing_token_exits_3() {
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("xp-ops");
    cmd.env("XP_OPS_DISTRO", "arch");
    cmd.args([
        "cloudflare",
        "provision",
        "--account-id",
        "acc",
        "--zone-id",
        "zone",
        "--hostname",
        "app.example.com",
        "--origin-url",
        "http://127.0.0.1:62416",
        "--dry-run",
    ]);
    cmd.assert().failure().code(3);
}

#[test]
fn init_writes_xray_config_under_test_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_string_lossy().to_string();

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("xp-ops");
    cmd.env("XP_OPS_DISTRO", "arch");
    cmd.args(["--root", &root, "init", "--init-system", "none"]);
    cmd.assert().success();

    let xray_cfg = tmp.path().join("etc/xray/config.json");
    let content = fs::read_to_string(xray_cfg).unwrap();
    assert!(content.contains("\"listen\": \"127.0.0.1:10085\""));
    assert!(content.contains("\"HandlerService\""));
    assert!(content.contains("\"StatsService\""));
    assert!(content.contains("\"inbounds\": []"));
}

#[tokio::test]
async fn cloudflare_provision_uses_mock_api_and_writes_files() {
    use wiremock::matchers::{header, method, path, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;

    let tunnel_id = "c1744f8b-faa1-48a4-9e5c-02ac921467fa";
    let dns_id = "372e67954025e0ba6aaa6d586b9e0b59";

    Mock::given(method("POST"))
        .and(path("/client/v4/accounts/acc/cfd_tunnel"))
        .and(header("authorization", "Bearer testtoken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
          "success": true,
          "errors": [],
          "result": {
            "id": tunnel_id,
            "credentials_file": {
              "AccountTag": "acc",
              "TunnelID": tunnel_id,
              "TunnelName": "xp",
              "TunnelSecret": "secret"
            }
          }
        })))
        .mount(&server)
        .await;

    Mock::given(method("PUT"))
        .and(path_regex(format!(
            "^/client/v4/accounts/acc/cfd_tunnel/{tunnel_id}/configurations$"
        )))
        .and(header("authorization", "Bearer testtoken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
          "success": true,
          "errors": [],
          "result": {}
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/client/v4/zones/zone/dns_records"))
        .and(header("authorization", "Bearer testtoken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
          "success": true,
          "errors": [],
          "result": { "id": dns_id }
        })))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_string_lossy().to_string();

    // Fake cloudflared binary to satisfy presence check.
    let cloudflared = tmp.path().join("usr/bin/cloudflared");
    fs::create_dir_all(cloudflared.parent().unwrap()).unwrap();
    fs::write(&cloudflared, b"#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut p = fs::metadata(&cloudflared).unwrap().permissions();
        p.set_mode(0o755);
        fs::set_permissions(&cloudflared, p).unwrap();
    }

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("xp-ops");
    cmd.env("XP_OPS_DISTRO", "arch");
    cmd.env("CLOUDFLARE_API_BASE_URL", server.uri());
    cmd.env("CLOUDFLARE_API_TOKEN", "testtoken");
    cmd.args([
        "--root",
        &root,
        "cloudflare",
        "provision",
        "--account-id",
        "acc",
        "--zone-id",
        "zone",
        "--hostname",
        "app.example.com",
        "--origin-url",
        "http://127.0.0.1:62416",
        "--no-enable",
    ]);

    let assert = cmd.assert().success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(!stderr.contains("testtoken"));

    let settings = tmp
        .path()
        .join("etc/xp-ops/cloudflare_tunnel/settings.json");
    let settings_raw = fs::read_to_string(settings).unwrap();
    assert!(settings_raw.contains(tunnel_id));
    assert!(settings_raw.contains(dns_id));

    let cfg = tmp.path().join("etc/cloudflared/config.yml");
    let cfg_raw = fs::read_to_string(cfg).unwrap();
    assert!(cfg_raw.contains(tunnel_id));
    assert!(cfg_raw.contains("credentials-file"));
}
