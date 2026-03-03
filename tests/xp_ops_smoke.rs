use base64::Engine as _;
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
fn mihomo_redact_file_masks_credentials() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("sub.txt");
    fs::write(
        &src,
        "vless://12345678-1234-1234-1234-123456789abc@edge.example.com:443?pbk=public_key_value&sid=0123456789abcdef#node-a\n",
    )
    .unwrap();

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("xp-ops");
    cmd.args(["mihomo", "redact", &src.to_string_lossy()]);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("vless://"))
        .stdout(predicate::str::contains("edge.example.com:443"))
        .stdout(predicate::str::contains("12345678-1234-1234-1234-123456789abc").not())
        .stdout(predicate::str::contains("public_key_value").not())
        .stdout(predicate::str::contains("0123456789abcdef").not());
}

#[test]
fn mihomo_redact_reads_stdin_when_source_missing() {
    let input = "proxies:\n  - name: edge\n    password: super-secret-value # keep comment\n";
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("xp-ops");
    cmd.args(["mihomo", "redact"]).write_stdin(input);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("proxies:"))
        .stdout(predicate::str::contains("# keep comment"))
        .stdout(predicate::str::contains("super-secret-value").not());
}

#[test]
fn mihomo_redact_reads_stdin_when_source_is_dash() {
    let input = "proxies:\n  - name: edge\n    password: super-secret-value # keep comment\n";
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("xp-ops");
    cmd.args(["mihomo", "redact", "-"]).write_stdin(input);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("proxies:"))
        .stdout(predicate::str::contains("# keep comment"))
        .stdout(predicate::str::contains("super-secret-value").not());
}

#[test]
fn mihomo_redact_prefers_source_over_stdin() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("config.yaml");
    fs::write(
        &src,
        "proxies:\n  - name: file-source\n    server: file.example.com\n    password: file-password\n",
    )
    .unwrap();

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("xp-ops");
    cmd.args(["mihomo", "redact", &src.to_string_lossy()])
        .write_stdin("proxies:\n  - name: stdin-source\n    server: stdin.example.com\n");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("file-source"))
        .stdout(predicate::str::contains("stdin-source").not())
        .stdout(predicate::str::contains("file-password").not());
}

#[tokio::test]
async fn mihomo_redact_url_fetch_supports_raw_base64_and_yaml() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    let raw_body = "vless://12345678-1234-1234-1234-123456789abc@raw.example.com:443?pbk=raw_public_key&sid=raw_short_id#raw\n";
    let b64_body = base64::engine::general_purpose::STANDARD.encode(
        "ss://2022-blake3-aes-128-gcm:AAAAAAAAAAAAAAAAAAAAAA==:BBBBBBBBBBBBBBBBBBBBBB==@b64.example.com:443#b64\n",
    );
    let yaml_body = "proxies:\n  - name: yaml\n    server: yaml.example.com\n    password: yaml-secret-password\n";

    Mock::given(method("GET"))
        .and(path("/raw"))
        .respond_with(ResponseTemplate::new(200).set_body_string(raw_body))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/base64"))
        .respond_with(ResponseTemplate::new(200).set_body_string(b64_body))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/yaml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(yaml_body))
        .mount(&server)
        .await;

    let mut raw_cmd = assert_cmd::cargo::cargo_bin_cmd!("xp-ops");
    raw_cmd.args(["mihomo", "redact", &format!("{}/raw", server.uri())]);
    raw_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("raw.example.com"))
        .stdout(predicate::str::contains("raw_public_key").not())
        .stdout(predicate::str::contains("raw_short_id").not());

    let mut base64_cmd = assert_cmd::cargo::cargo_bin_cmd!("xp-ops");
    base64_cmd.args(["mihomo", "redact", &format!("{}/base64", server.uri())]);
    base64_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("ss://2022-blake3-aes-128-gcm"))
        .stdout(
            predicate::str::contains("AAAAAAAAAAAAAAAAAAAAAA==:BBBBBBBBBBBBBBBBBBBBBB==").not(),
        );

    let mut yaml_cmd = assert_cmd::cargo::cargo_bin_cmd!("xp-ops");
    yaml_cmd.args(["mihomo", "redact", &format!("{}/yaml", server.uri())]);
    yaml_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("yaml.example.com"))
        .stdout(predicate::str::contains("yaml-secret-password").not());
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
fn deploy_join_dry_run_succeeds_without_join_side_effects() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // Satisfy `xp join` dry-run prerequisite: `/usr/local/bin/xp` exists.
    let xp_bin = root.join("usr/local/bin/xp");
    fs::create_dir_all(xp_bin.parent().unwrap()).unwrap();
    fs::write(&xp_bin, b"#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut p = fs::metadata(&xp_bin).unwrap().permissions();
        p.set_mode(0o755);
        fs::set_permissions(&xp_bin, p).unwrap();
    }

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("xp-ops");
    cmd.env("XP_OPS_DISTRO", "arch");
    cmd.args([
        "--root",
        &root.to_string_lossy(),
        "deploy",
        "--node-name",
        "node1",
        "--access-host",
        "node1.example.invalid",
        "--join-token",
        "join-token",
        "--no-cloudflare",
        "--api-base-url",
        "https://api.example.invalid",
        "--dry-run",
    ]);

    cmd.assert().success();
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
    assert!(content.contains("\"HandlerService\""));
    assert!(content.contains("\"StatsService\""));
    assert!(content.contains("\"port\": 10085"));
    assert!(content.contains("\"protocol\": \"dokodemo-door\""));
    assert!(content.contains("\"tag\": \"api\""));
    assert!(content.contains("\"routing\""));
}

#[test]
#[cfg(unix)]
fn preflight_fails_fast_when_fs_not_writable() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // Make the root `/etc` exist but read-only, so preflight fails before attempting writes.
    let etc = root.join("etc");
    fs::create_dir_all(&etc).unwrap();
    fs::set_permissions(&etc, fs::Permissions::from_mode(0o555)).unwrap();

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("xp-ops");
    cmd.env("CLOUDFLARE_API_TOKEN", "tok");
    cmd.args([
        "--root",
        &root.to_string_lossy(),
        "cloudflare",
        "token",
        "set",
        "--from-env",
        "CLOUDFLARE_API_TOKEN",
    ]);

    cmd.assert()
        .failure()
        .code(6)
        .stderr(predicate::str::contains("preflight_failed"))
        .stderr(predicate::str::contains("etc/xp-ops"))
        .stderr(predicate::str::contains("Permission denied"));
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
