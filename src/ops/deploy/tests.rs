use super::*;
use axum::{Router, http::StatusCode, routing::get};
use clap::Parser;
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const VALID_ADMIN_TOKEN_HASH: &str = "$argon2id$v=19$m=65536,t=3,p=1$TqOws+M/ypxKCmnVcbWAdg$VlLbEUvXvoESmlktijJp9QYD/jJklIIljA1vuce9P+k";

fn read_env(paths: &Paths) -> String {
    fs::read_to_string(paths.etc_xp_env()).unwrap()
}

fn empty_managed_defaults() -> ManagedDefaultsWriteValues<'static> {
    ManagedDefaultsWriteValues {
        vless_canary_acme_contact_email: None,
        default_vless_port: None,
        default_vless_server_names: None,
        default_vless_fingerprint: None,
        default_ss_port: None,
    }
}

#[test]
fn ensure_xp_env_admin_token_hash_keeps_xray_defaults_on_second_run() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());

    fs::create_dir_all(paths.etc_xp_dir()).unwrap();
    fs::write(
        paths.etc_xp_env(),
        format!("XP_ADMIN_TOKEN_HASH={VALID_ADMIN_TOKEN_HASH}\n"),
    )
    .unwrap();

    ensure_xp_env_admin_token_hash_bootstrap(
        &paths,
        Mode::Real,
        "node-1",
        "example.com",
        "https://example.com",
        false,
        "",
        false,
        &empty_managed_defaults(),
        false,
    )
    .unwrap();
    ensure_xp_env_admin_token_hash_bootstrap(
        &paths,
        Mode::Real,
        "node-1",
        "example.com",
        "https://example.com",
        false,
        "",
        false,
        &empty_managed_defaults(),
        false,
    )
    .unwrap();

    let env = read_env(&paths);
    assert!(env.contains(VALID_ADMIN_TOKEN_HASH));
    assert!(env.contains("XP_DATA_DIR="));
    assert!(env.contains("XP_XRAY_API_ADDR="));
    assert!(env.contains("XP_XRAY_HEALTH_INTERVAL_SECS="));
    assert!(env.contains("XP_XRAY_HEALTH_FAILS_BEFORE_DOWN="));
    assert!(env.contains("XP_XRAY_RESTART_MODE="));
    assert!(env.contains("XP_XRAY_RESTART_COOLDOWN_SECS="));
    assert!(env.contains("XP_XRAY_RESTART_TIMEOUT_SECS="));
    assert!(env.contains("XP_XRAY_SYSTEMD_UNIT="));
    assert!(env.contains("XP_XRAY_OPENRC_SERVICE="));
    assert!(env.contains("XP_CLOUDFLARED_HEALTH_INTERVAL_SECS="));
    assert!(env.contains("XP_CLOUDFLARED_HEALTH_FAILS_BEFORE_DOWN="));
    assert!(env.contains("XP_CLOUDFLARED_MONITOR_MODE="));
    assert!(env.contains("XP_CLOUDFLARED_RESTART_MODE="));
    assert!(env.contains("XP_CLOUDFLARED_RESTART_COOLDOWN_SECS="));
    assert!(env.contains("XP_CLOUDFLARED_RESTART_TIMEOUT_SECS="));
    assert!(env.contains("XP_CLOUDFLARED_SYSTEMD_UNIT="));
    assert!(env.contains("XP_CLOUDFLARED_OPENRC_SERVICE="));
    assert!(!env.contains("XP_IP_GEO_ENABLED="));
}

#[test]
fn ensure_xp_env_admin_token_hash_preserves_user_xray_overrides() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());

    fs::create_dir_all(paths.etc_xp_dir()).unwrap();
    fs::write(
        paths.etc_xp_env(),
        format!(
            "XP_ADMIN_TOKEN_HASH={VALID_ADMIN_TOKEN_HASH}\n\
XP_DATA_DIR=/custom/data\n\
XP_XRAY_API_ADDR=127.0.0.1:12345\n\
XP_XRAY_RESTART_MODE=systemd\n\
XP_XRAY_SYSTEMD_UNIT=custom-xray.service\n\
XP_XRAY_OPENRC_SERVICE=custom-xray\n\
XP_XRAY_CUSTOM=keep-me\n",
        ),
    )
    .unwrap();

    ensure_xp_env_admin_token_hash_bootstrap(
        &paths,
        Mode::Real,
        "node-1",
        "example.com",
        "https://example.com",
        false,
        "",
        false,
        &empty_managed_defaults(),
        false,
    )
    .unwrap();

    let env = read_env(&paths);
    assert!(env.contains("XP_DATA_DIR=/custom/data"));
    assert!(env.contains("XP_XRAY_API_ADDR=127.0.0.1:12345"));
    assert!(env.contains("XP_XRAY_RESTART_MODE=systemd"));
    assert!(env.contains("XP_XRAY_SYSTEMD_UNIT=custom-xray.service"));
    assert!(env.contains("XP_XRAY_OPENRC_SERVICE=custom-xray"));
    assert!(env.contains("XP_XRAY_CUSTOM=keep-me"));
}

#[test]
fn ensure_xp_env_admin_token_hash_preserves_cloudflared_restart_none_opt_out() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());

    fs::create_dir_all(paths.etc_xp_dir()).unwrap();
    fs::write(
        paths.etc_xp_env(),
        format!(
            "XP_ADMIN_TOKEN_HASH={VALID_ADMIN_TOKEN_HASH}\n\
XP_CLOUDFLARED_RESTART_MODE=none\n",
        ),
    )
    .unwrap();

    ensure_xp_env_admin_token_hash_bootstrap(
        &paths,
        Mode::Real,
        "node-1",
        "example.com",
        "https://example.com",
        false,
        "",
        false,
        &empty_managed_defaults(),
        false,
    )
    .unwrap();

    let env = read_env(&paths);
    assert!(env.contains("XP_CLOUDFLARED_RESTART_MODE=none"));
    assert!(!env.contains("XP_CLOUDFLARED_MONITOR_MODE="));
}

#[test]
fn ensure_xp_env_admin_token_hash_writes_managed_default_endpoint_keys() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());

    fs::create_dir_all(paths.etc_xp_dir()).unwrap();
    fs::write(
        paths.etc_xp_env(),
        format!("XP_ADMIN_TOKEN_HASH={VALID_ADMIN_TOKEN_HASH}\n"),
    )
    .unwrap();

    let managed_defaults = ManagedDefaultsWriteValues {
        vless_canary_acme_contact_email: Some(Cow::Borrowed("ops@example.com")),
        default_vless_port: Some(Cow::Borrowed("53842")),
        default_vless_server_names: Some(Cow::Borrowed("cdn-a.example.test,cdn-b.example.test")),
        default_vless_fingerprint: Some(Cow::Borrowed("chrome")),
        default_ss_port: Some(Cow::Borrowed("53843")),
    };

    ensure_xp_env_admin_token_hash_bootstrap(
        &paths,
        Mode::Real,
        "node-1",
        "example.com",
        "https://example.com",
        false,
        "",
        false,
        &managed_defaults,
        false,
    )
    .unwrap();

    let env = read_env(&paths);
    assert!(env.contains("XP_VLESS_CANARY_ACME_CONTACT_EMAIL='ops@example.com'"));
    assert!(env.contains("XP_DEFAULT_VLESS_PORT='53842'"));
    assert!(env.contains("XP_DEFAULT_VLESS_SERVER_NAMES='cdn-a.example.test,cdn-b.example.test'"));
    assert!(env.contains("XP_DEFAULT_VLESS_FINGERPRINT='chrome'"));
    assert!(env.contains("XP_DEFAULT_SS_PORT='53843'"));
}

#[test]
fn ensure_xp_env_admin_token_hash_preserves_existing_vless_canary_overrides() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());

    fs::create_dir_all(paths.etc_xp_dir()).unwrap();
    fs::write(
        paths.etc_xp_env(),
        format!(
            "XP_ADMIN_TOKEN_HASH={VALID_ADMIN_TOKEN_HASH}\n\
XP_VLESS_CANARY_BIND=127.0.0.1:49043\n\
XP_VLESS_CANARY_ACME_DIRECTORY_URL=https://acme-staging-v02.api.letsencrypt.org/directory\n\
XP_VLESS_CANARY_ACME_CONTACT_EMAIL=ops@example.com\n\
XP_VLESS_CANARY_CLOUDFLARE_TOKEN_FILE=/custom/token\n\
XP_VLESS_CANARY_CLOUDFLARE_ZONE_ID=zone-123\n",
        ),
    )
    .unwrap();

    ensure_xp_env_admin_token_hash_bootstrap(
        &paths,
        Mode::Real,
        "node-1",
        "example.com",
        "https://example.com",
        false,
        "",
        false,
        &empty_managed_defaults(),
        false,
    )
    .unwrap();

    let env = read_env(&paths);
    assert!(env.contains("XP_VLESS_CANARY_BIND='127.0.0.1:49043'"));
    assert!(env.contains(
            "XP_VLESS_CANARY_ACME_DIRECTORY_URL='https://acme-staging-v02.api.letsencrypt.org/directory'"
        ));
    assert!(env.contains("XP_VLESS_CANARY_ACME_CONTACT_EMAIL='ops@example.com'"));
    assert!(env.contains("XP_VLESS_CANARY_CLOUDFLARE_TOKEN_FILE='/custom/token'"));
    assert!(env.contains("XP_VLESS_CANARY_CLOUDFLARE_ZONE_ID='zone-123'"));
}

#[test]
fn ensure_xp_env_admin_token_hash_preserves_existing_managed_default_endpoint_keys() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());

    fs::create_dir_all(paths.etc_xp_dir()).unwrap();
    fs::write(
        paths.etc_xp_env(),
        format!(
            "XP_ADMIN_TOKEN_HASH={VALID_ADMIN_TOKEN_HASH}\n\
XP_DEFAULT_VLESS_PORT=53842\n\
XP_DEFAULT_VLESS_SERVER_NAMES=cdn-a.example.test,cdn-b.example.test\n\
XP_DEFAULT_VLESS_FINGERPRINT=chrome\n\
XP_DEFAULT_SS_PORT=53843\n",
        ),
    )
    .unwrap();

    ensure_xp_env_admin_token_hash_bootstrap(
        &paths,
        Mode::Real,
        "node-1",
        "example.com",
        "https://example.com",
        false,
        "",
        false,
        &empty_managed_defaults(),
        false,
    )
    .unwrap();

    let env = read_env(&paths);
    assert!(env.contains("XP_DEFAULT_VLESS_PORT='53842'"));
    assert!(env.contains("XP_DEFAULT_VLESS_SERVER_NAMES='cdn-a.example.test,cdn-b.example.test'"));
    assert!(env.contains("XP_DEFAULT_VLESS_FINGERPRINT='chrome'"));
    assert!(env.contains("XP_DEFAULT_SS_PORT='53843'"));
}

#[test]
fn bootstrap_geo_opt_in_writes_true_and_omission_preserves_existing_value() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());

    fs::create_dir_all(paths.etc_xp_dir()).unwrap();
    fs::write(
        paths.etc_xp_env(),
        format!("XP_ADMIN_TOKEN_HASH={VALID_ADMIN_TOKEN_HASH}\nXP_IP_GEO_ENABLED=false\n"),
    )
    .unwrap();

    ensure_xp_env_admin_token_hash_bootstrap(
        &paths,
        Mode::Real,
        "node-1",
        "example.com",
        "https://example.com",
        false,
        "",
        false,
        &empty_managed_defaults(),
        false,
    )
    .unwrap();
    assert!(read_env(&paths).contains("XP_IP_GEO_ENABLED=false"));

    ensure_xp_env_admin_token_hash_bootstrap(
        &paths,
        Mode::Real,
        "node-1",
        "example.com",
        "https://example.com",
        false,
        "",
        true,
        &empty_managed_defaults(),
        false,
    )
    .unwrap();
    let env = read_env(&paths);
    assert!(env.contains("XP_IP_GEO_ENABLED=true"));
    assert!(!env.contains("XP_IP_GEO_ENABLED=false"));
}

#[test]
fn join_geo_opt_in_writes_true_and_omission_preserves_existing_value() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());

    fs::create_dir_all(paths.etc_xp_dir()).unwrap();
    fs::write(
        paths.etc_xp_env(),
        format!("XP_ADMIN_TOKEN_HASH={VALID_ADMIN_TOKEN_HASH}\nXP_IP_GEO_ENABLED=false\n"),
    )
    .unwrap();

    ensure_xp_env_admin_token_hash_join(
        &paths,
        Mode::Real,
        VALID_ADMIN_TOKEN_HASH,
        "node-1",
        "example.com",
        "https://example.com",
        false,
        "",
        false,
        &empty_managed_defaults(),
        false,
    )
    .unwrap();
    assert!(read_env(&paths).contains("XP_IP_GEO_ENABLED=false"));

    ensure_xp_env_admin_token_hash_join(
        &paths,
        Mode::Real,
        VALID_ADMIN_TOKEN_HASH,
        "node-1",
        "example.com",
        "https://example.com",
        false,
        "",
        true,
        &empty_managed_defaults(),
        false,
    )
    .unwrap();
    let env = read_env(&paths);
    assert!(env.contains("XP_IP_GEO_ENABLED=true"));
    assert!(!env.contains("XP_IP_GEO_ENABLED=false"));
}

#[test]
fn resolve_managed_defaults_write_values_preserves_existing_endpoint_settings() {
    let parsed = crate::ops::xp_env::parse_xp_env(Some(
        "XP_VLESS_CANARY_ACME_CONTACT_EMAIL=ops@example.com\n\
XP_DEFAULT_VLESS_PORT=53842\n\
XP_DEFAULT_VLESS_SERVER_NAMES=cdn-a.example.test,cdn-b.example.test\n\
XP_DEFAULT_VLESS_FINGERPRINT=chrome\n\
XP_DEFAULT_SS_PORT=53843\n"
            .to_string(),
    ));
    let defaults = empty_managed_defaults();
    let resolved = resolve_managed_defaults_write_values(&parsed, &defaults);

    assert_eq!(
        resolved.vless_canary_acme_contact_email.as_deref(),
        Some("ops@example.com")
    );
    assert_eq!(resolved.default_vless_port.as_deref(), Some("53842"));
    assert_eq!(
        resolved.default_vless_server_names.as_deref(),
        Some("cdn-a.example.test,cdn-b.example.test")
    );
    assert_eq!(
        resolved.default_vless_fingerprint.as_deref(),
        Some("chrome")
    );
    assert_eq!(resolved.default_ss_port.as_deref(), Some("53843"));
}

#[test]
fn ensure_runtime_token_file_writes_xp_readable_token() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());

    ensure_runtime_token_file(
        &paths,
        Mode::Real,
        "  test-token  ",
        Path::new(crate::config::DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE),
    )
    .unwrap();

    let written = fs::read_to_string(
        paths.map_abs(Path::new(crate::config::DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE)),
    )
    .unwrap();
    assert_eq!(written, "test-token\n");
}

#[test]
fn ensure_runtime_token_file_honors_custom_canary_path() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());

    ensure_runtime_token_file(
        &paths,
        Mode::Real,
        "test-token",
        Path::new("/custom/nested/token"),
    )
    .unwrap();

    let written = fs::read_to_string(paths.map_abs(Path::new("/custom/nested/token"))).unwrap();
    assert_eq!(written, "test-token\n");
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn build_plan_cloudflare_token_missing_error_is_actionable() {
    let _lock = crate::ops::util::ENV_LOCK.lock().unwrap();
    unsafe { std::env::remove_var("CLOUDFLARE_API_TOKEN") };

    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());

    let xp_bin = tmp.path().join("xp");
    fs::write(&xp_bin, b"dummy").unwrap();

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
        account_id: Some("acc".to_string()),
        zone_id: Some("zone".to_string()),
        hostname: Some(xp_test_fixtures::host_fixture553().to_owned()),
        tunnel_name: None,
        origin_url: None,
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
        cloudflare_token: None,
        cloudflare_token_stdin: false,
        cloudflare_token_stdin_value: None,
        api_base_url: xp_test_fixtures::none(),
        xray_version: "latest".to_string(),
        enable_services_toggle: crate::ops::cli::EnableServicesToggle {
            enable_services: false,
            no_enable_services: true,
        },
        yes: false,
        overwrite_existing: false,
        non_interactive: true,
        dry_run: true,
    };

    let plan = build_plan(&paths, &args).await.unwrap();
    assert!(
        plan.errors
            .iter()
            .any(|e| e.contains("--cloudflare-token") && e.contains("CLOUDFLARE_API_TOKEN")),
        "expected actionable token missing error, got: {:?}",
        plan.errors
    );
    assert!(
        !plan.errors.iter().any(|e| e.contains("token_missing")),
        "should not emit raw token_missing error string: {:?}",
        plan.errors
    );
}

#[tokio::test]
async fn build_plan_allows_default_vless_port_without_server_names() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let xp_bin = tmp.path().join("xp");
    fs::write(&xp_bin, b"dummy").unwrap();

    let args = DeployArgs {
        xp_bin: Some(xp_bin),
        node_name: xp_test_fixtures::label_node1_variant2().to_owned(),
        access_host: xp_test_fixtures::host_fixture552().to_owned(),
        cloudflare_toggle: crate::ops::cli::CloudflareToggle::default(),
        ddns_toggle: crate::ops::cli::DdnsToggle::default(),
        ip_geo_enabled: false,
        account_id: None,
        zone_id: None,
        hostname: xp_test_fixtures::none(),
        tunnel_name: None,
        origin_url: None,
        migrate_existing_tunnel: false,
        ddns_zone_id: None,
        vless_canary_acme_contact_email: None,
        default_vless_port: Some(53842),
        default_vless_server_names: None,
        default_vless_fingerprint: None,
        default_ss_port: None,
        join_token: None,
        join_token_stdin: false,
        join_token_stdin_value: None,
        cloudflare_token: None,
        cloudflare_token_stdin: false,
        cloudflare_token_stdin_value: None,
        api_base_url: Some(xp_test_fixtures::service_fixture564().to_owned()),
        xray_version: "latest".to_string(),
        enable_services_toggle: crate::ops::cli::EnableServicesToggle {
            enable_services: false,
            no_enable_services: false,
        },
        yes: false,
        overwrite_existing: false,
        non_interactive: true,
        dry_run: true,
    };

    let plan = build_plan(&paths, &args).await.unwrap();
    assert!(
        !plan
            .errors
            .iter()
            .any(|e| e.contains("--default-vless-server-names")),
        "default vless server names should be optional for managed SNI, got: {:?}",
        plan.errors
    );
}

#[tokio::test]
async fn build_plan_rejects_zero_managed_default_ports() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let xp_bin = tmp.path().join("xp");
    fs::write(&xp_bin, b"dummy").unwrap();

    let args = DeployArgs {
        xp_bin: Some(xp_bin),
        node_name: xp_test_fixtures::label_node1_variant2().to_owned(),
        access_host: xp_test_fixtures::host_fixture552().to_owned(),
        cloudflare_toggle: crate::ops::cli::CloudflareToggle::default(),
        ddns_toggle: crate::ops::cli::DdnsToggle::default(),
        ip_geo_enabled: false,
        account_id: None,
        zone_id: None,
        hostname: xp_test_fixtures::none(),
        tunnel_name: None,
        origin_url: None,
        migrate_existing_tunnel: false,
        ddns_zone_id: None,
        vless_canary_acme_contact_email: None,
        default_vless_port: Some(0),
        default_vless_server_names: Some("cdn-a.example.test".to_string()),
        default_vless_fingerprint: None,
        default_ss_port: Some(0),
        join_token: None,
        join_token_stdin: false,
        join_token_stdin_value: None,
        cloudflare_token: None,
        cloudflare_token_stdin: false,
        cloudflare_token_stdin_value: None,
        api_base_url: Some(xp_test_fixtures::service_fixture564().to_owned()),
        xray_version: "latest".to_string(),
        enable_services_toggle: crate::ops::cli::EnableServicesToggle {
            enable_services: false,
            no_enable_services: false,
        },
        yes: false,
        overwrite_existing: false,
        non_interactive: true,
        dry_run: true,
    };

    let plan = build_plan(&paths, &args).await.unwrap();
    assert!(
        plan.errors
            .iter()
            .any(|e| e.contains("--default-vless-port") && e.contains("invalid port: 0")),
        "expected zero VLESS port validation error, got: {:?}",
        plan.errors
    );
    assert!(
        plan.errors
            .iter()
            .any(|e| e.contains("--default-ss-port") && e.contains("invalid port: 0")),
        "expected zero SS port validation error, got: {:?}",
        plan.errors
    );
}

#[tokio::test]
async fn build_plan_rejects_zero_managed_default_ports_from_existing_env() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let xp_bin = tmp.path().join("xp");
    fs::write(&xp_bin, b"dummy").unwrap();
    fs::create_dir_all(paths.etc_xp_dir()).unwrap();
    fs::write(
            paths.etc_xp_env(),
            "XP_DEFAULT_VLESS_PORT=0\nXP_DEFAULT_VLESS_SERVER_NAMES=cdn-a.example.test\nXP_DEFAULT_SS_PORT=0\n",
        )
        .unwrap();

    let args = DeployArgs {
        xp_bin: Some(xp_bin),
        node_name: xp_test_fixtures::label_node1_variant2().to_owned(),
        access_host: xp_test_fixtures::host_fixture552().to_owned(),
        cloudflare_toggle: crate::ops::cli::CloudflareToggle::default(),
        ddns_toggle: crate::ops::cli::DdnsToggle::default(),
        ip_geo_enabled: false,
        account_id: None,
        zone_id: None,
        hostname: xp_test_fixtures::none(),
        tunnel_name: None,
        origin_url: None,
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
        cloudflare_token: None,
        cloudflare_token_stdin: false,
        cloudflare_token_stdin_value: None,
        api_base_url: Some(xp_test_fixtures::service_fixture564().to_owned()),
        xray_version: "latest".to_string(),
        enable_services_toggle: crate::ops::cli::EnableServicesToggle {
            enable_services: false,
            no_enable_services: false,
        },
        yes: false,
        overwrite_existing: false,
        non_interactive: true,
        dry_run: true,
    };

    let plan = build_plan(&paths, &args).await.unwrap();
    assert!(
        plan.errors
            .iter()
            .any(|e| e.contains("existing XP_DEFAULT_VLESS_PORT") && e.contains("invalid port: 0")),
        "expected existing VLESS env validation error, got: {:?}",
        plan.errors
    );
    assert!(
        plan.errors
            .iter()
            .any(|e| e.contains("existing XP_DEFAULT_SS_PORT") && e.contains("invalid port: 0")),
        "expected existing SS env validation error, got: {:?}",
        plan.errors
    );
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn build_plan_detects_token_need_from_existing_managed_vless_env() {
    let _lock = crate::ops::util::ENV_LOCK.lock().unwrap();
    unsafe { std::env::remove_var("CLOUDFLARE_API_TOKEN") };

    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let xp_bin = tmp.path().join("xp");
    fs::write(&xp_bin, b"dummy").unwrap();
    fs::create_dir_all(paths.etc_xp_dir()).unwrap();
    fs::write(
        paths.etc_xp_env(),
        "XP_DEFAULT_VLESS_PORT=53842\nXP_DEFAULT_VLESS_SERVER_NAMES=cdn-a.example.test\n",
    )
    .unwrap();

    let args = DeployArgs {
        xp_bin: Some(xp_bin),
        node_name: xp_test_fixtures::label_node1_variant2().to_owned(),
        access_host: xp_test_fixtures::host_fixture552().to_owned(),
        cloudflare_toggle: crate::ops::cli::CloudflareToggle::default(),
        ddns_toggle: crate::ops::cli::DdnsToggle::default(),
        ip_geo_enabled: false,
        account_id: None,
        zone_id: None,
        hostname: xp_test_fixtures::none(),
        tunnel_name: None,
        origin_url: None,
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
        cloudflare_token: None,
        cloudflare_token_stdin: false,
        cloudflare_token_stdin_value: None,
        api_base_url: Some(xp_test_fixtures::service_fixture564().to_owned()),
        xray_version: "latest".to_string(),
        enable_services_toggle: crate::ops::cli::EnableServicesToggle {
            enable_services: false,
            no_enable_services: false,
        },
        yes: false,
        overwrite_existing: false,
        non_interactive: true,
        dry_run: true,
    };

    let plan = build_plan(&paths, &args).await.unwrap();
    assert!(
        plan.errors
            .iter()
            .any(|e| e.contains("cloudflare token missing")),
        "expected existing managed vless env to require cloudflare token, got: {:?}",
        plan.errors
    );
}

#[test]
fn post_join_service_activation_orders_and_waits_each_service() {
    assert_eq!(managed_service_names(false), ["xray", "xp"]);
    assert_eq!(managed_service_names(true), ["xray", "xp", "cloudflared"]);
    assert_eq!(
        managed_service_activation_steps(true),
        vec![
            ManagedServiceActivationStep::Enable("xray"),
            ManagedServiceActivationStep::StartOrRestart("xray"),
            ManagedServiceActivationStep::WaitReady("xray"),
            ManagedServiceActivationStep::Enable("xp"),
            ManagedServiceActivationStep::StartOrRestart("xp"),
            ManagedServiceActivationStep::WaitReady("xp"),
            ManagedServiceActivationStep::Enable("cloudflared"),
            ManagedServiceActivationStep::StartOrRestart("cloudflared"),
            ManagedServiceActivationStep::WaitReady("cloudflared"),
        ]
    );
}

#[test]
fn service_activation_rejects_command_failures() {
    assert!(ensure_service_command_succeeded("xp", "enable", Ok(true)).is_ok());

    let status_error = ensure_service_command_succeeded("xp", "enable", Ok(false))
        .expect_err("non-zero service command must fail deploy");
    assert_eq!(status_error.code, 6);
    assert!(
        status_error
            .message
            .contains("service_failed: xp enable failed")
    );

    let execution_error = ensure_service_command_succeeded(
        "xp",
        "start/restart",
        Err(io::Error::other("systemctl unavailable")),
    )
    .expect_err("unavailable service command must fail deploy");
    assert_eq!(execution_error.code, 6);
    assert!(
        execution_error
            .message
            .contains("service_failed: xp start/restart: systemctl unavailable")
    );
}

#[test]
fn existing_cluster_metadata_marks_deploy_as_post_join_recovery() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let metadata = paths.map_abs(Path::new("/var/lib/xp/data/cluster/metadata.json"));

    assert!(!cluster_metadata_exists(&paths));
    fs::create_dir_all(metadata.parent().unwrap()).unwrap();
    fs::write(metadata, "{}\n").unwrap();
    assert!(cluster_metadata_exists(&paths));
}

#[test]
fn deploy_cli_uses_explicit_ip_geo_flag_name() {
    let cli = crate::ops::cli::Cli::try_parse_from([
        "xp-ops",
        "deploy",
        "--node-name",
        "node-1",
        "--access-host",
        "node-1.example.net",
        "--no-cloudflare",
        "--no-enable-services",
        "--ip-geo",
        "--dry-run",
    ])
    .unwrap();

    let crate::ops::cli::Command::Deploy(args) = cli.command.unwrap() else {
        panic!("expected deploy command");
    };
    assert!(args.ip_geo_enabled);
}

#[tokio::test]
async fn post_join_public_api_probe_accepts_only_http_200() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock)
        .await;

    let client = reqwest::Client::builder().build().unwrap();
    let status = wait_for_post_join_public_api_health_with_client(
        &client,
        &format!("{}/health", mock.uri()),
        1,
        Duration::from_millis(1),
    )
    .await
    .unwrap();
    assert_eq!(status.as_u16(), 200);
}

#[tokio::test]
async fn post_join_public_api_probe_rejects_non_200_edge_responses() {
    for status in [502, 530] {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(status))
            .mount(&mock)
            .await;

        let client = reqwest::Client::builder().build().unwrap();
        let err = wait_for_post_join_public_api_health_with_client(
            &client,
            &format!("{}/health", mock.uri()),
            1,
            Duration::from_millis(1),
        )
        .await
        .unwrap_err();
        assert!(
            err.message.contains("post_join_health_failed")
                && err.message.contains(&format!("http {status}")),
            "unexpected error: {}",
            err.message
        );
    }
}

#[tokio::test]
async fn post_join_public_api_probe_retries_transport_errors_until_healthy() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let (ready_tx, ready_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(60)).await;
        let app = Router::new().route("/health", get(|| async { (StatusCode::OK, "ready") }));
        let listener = TcpListener::bind(addr).await.unwrap();
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = ready_rx.await;
            })
            .await
            .unwrap();
    });

    let client = reqwest::Client::builder().build().unwrap();
    let status = wait_for_post_join_public_api_health_with_client(
        &client,
        &format!("http://{addr}/health"),
        10,
        Duration::from_millis(20),
    )
    .await
    .unwrap();

    let _ = ready_tx.send(());
    assert_eq!(status.as_u16(), 200);
}

#[tokio::test]
async fn post_join_public_api_probe_reports_timeouts_as_recoverable_failure() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(50)))
        .mount(&mock)
        .await;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(1))
        .build()
        .unwrap();
    let err = wait_for_post_join_public_api_health_with_client(
        &client,
        &format!("{}/health", mock.uri()),
        1,
        Duration::from_millis(1),
    )
    .await
    .unwrap_err();
    assert!(err.message.contains("post_join_health_failed"));
}
