use super::*;
use std::fs;

fn test_paths() -> (tempfile::TempDir, Paths) {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    (tmp, paths)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

#[test]
fn ctrl_q_exits_when_not_dirty() {
    let (_tmp, paths) = test_paths();
    let mut app = App::new(&paths);
    let action = app.handle_key(ctrl('q'));
    assert!(matches!(action, Some(AppAction::Quit)));
}

#[test]
fn ctrl_q_enters_confirm_quit_when_dirty() {
    let (_tmp, paths) = test_paths();
    let mut app = App::new(&paths);
    app.node_name.push('x');

    let action = app.handle_key(ctrl('q'));
    assert!(action.is_none());
    assert_eq!(app.mode, UiMode::ConfirmQuit);
}

#[test]
fn confirm_quit_cancel_returns_to_nav() {
    let (_tmp, paths) = test_paths();
    let mut app = App::new(&paths);
    app.node_name.push('x');
    let _ = app.handle_key(ctrl('q'));
    assert_eq!(app.mode, UiMode::ConfirmQuit);

    let action = app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(action.is_none());
    assert_eq!(app.mode, UiMode::Nav);
}

#[test]
fn confirm_quit_ctrl_s_means_save_and_exit() {
    let (_tmp, paths) = test_paths();
    let mut app = App::new(&paths);
    app.node_name.push('x');
    let _ = app.handle_key(ctrl('q'));

    let action = app.handle_key(ctrl('s'));
    assert!(matches!(action, Some(AppAction::Save { exit_after: true })));
}

#[test]
fn plain_q_does_not_quit() {
    let (_tmp, paths) = test_paths();
    let mut app = App::new(&paths);
    let action = app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
    assert!(action.is_none());
    assert_eq!(app.mode, UiMode::Nav);
}

#[test]
fn save_tui_config_omits_legacy_save_token_field() {
    let (tmp, paths) = test_paths();
    let values = AppValues {
        node_name: "node-1".to_string(),
        access_host: "node-1.example.net".to_string(),
        cloudflare_enabled: true,
        ddns_enabled: false,
        ddns_zone_id: None,
        account_id: Some("acc".to_string()),
        zone_id: Some("zone".to_string()),
        hostname: Some("node-1.example.com".to_string()),
        origin_url: Some("http://127.0.0.1:62416".to_string()),
        api_base_url: None,
        vless_canary_acme_contact_email: Some("ops@example.com".to_string()),
        default_vless_port: Some(443),
        default_vless_server_names: Some(
            "public.sn.files.1drv.com,public.bn.files.1drv.com".to_string(),
        ),
        default_vless_fingerprint: Some("chrome".to_string()),
        default_ss_port: Some(53843),
        xray_version: "latest".to_string(),
        cloudflare_token: String::new(),
        enable_services: true,
        dry_run: false,
    };

    save_tui_config(&paths, &values).unwrap();
    let raw = fs::read_to_string(tmp.path().join("etc/xp-ops/deploy/settings.json")).unwrap();
    assert!(raw.contains("\"node_name\""));
    assert!(!raw.contains("save_token"));
}

#[test]
fn load_tui_config_supports_public_domain_alias() {
    let (tmp, paths) = test_paths();
    let p = tmp.path().join("etc/xp-ops/deploy/settings.json");
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, r#"{ "public_domain": "node-1.example.net" }"#).unwrap();

    let app = App::new(&paths);
    assert_eq!(app.access_host, "node-1.example.net");
}

#[test]
fn save_tui_config_persists_managed_default_fields() {
    let (tmp, paths) = test_paths();
    let values = AppValues {
        node_name: "node-1".to_string(),
        access_host: "node-1.example.net".to_string(),
        cloudflare_enabled: true,
        ddns_enabled: true,
        ddns_zone_id: Some("zone-ddns".to_string()),
        account_id: Some("acc".to_string()),
        zone_id: Some("zone".to_string()),
        hostname: Some("node-1.example.com".to_string()),
        origin_url: Some("http://127.0.0.1:62416".to_string()),
        api_base_url: None,
        vless_canary_acme_contact_email: Some("ops@example.com".to_string()),
        default_vless_port: Some(443),
        default_vless_server_names: Some(
            "public.sn.files.1drv.com,public.bn.files.1drv.com".to_string(),
        ),
        default_vless_fingerprint: Some("chrome".to_string()),
        default_ss_port: Some(53843),
        xray_version: "latest".to_string(),
        cloudflare_token: String::new(),
        enable_services: true,
        dry_run: false,
    };

    save_tui_config(&paths, &values).unwrap();
    let raw = fs::read_to_string(tmp.path().join("etc/xp-ops/deploy/settings.json")).unwrap();
    assert!(raw.contains("\"vless_canary_acme_contact_email\": \"ops@example.com\""));
    assert!(raw.contains("\"default_vless_port\": \"443\""));
    assert!(raw.contains("\"default_vless_server_names\""));
    assert!(raw.contains("\"default_vless_fingerprint\": \"chrome\""));
    assert!(raw.contains("\"default_ss_port\": \"53843\""));
}

#[test]
fn to_values_rejects_invalid_managed_default_port() {
    let (_tmp, paths) = test_paths();
    let mut app = App::new(&paths);
    app.default_vless_port = "not-a-port".to_string();

    let err = app.to_values().unwrap_err();
    assert_eq!(err.code, 2);
    assert!(err.message.contains("default_vless_port"));
}

#[test]
fn to_values_rejects_zero_managed_default_port() {
    let (_tmp, paths) = test_paths();
    let mut app = App::new(&paths);
    app.default_ss_port = "0".to_string();

    let err = app.to_values().unwrap_err();
    assert_eq!(err.code, 2);
    assert!(err.message.contains("default_ss_port"));
    assert!(err.message.contains("between 1 and 65535"));
}

#[test]
fn save_token_empty_keeps_existing_token_unchanged() {
    let (tmp, paths) = test_paths();
    let token_path = tmp.path().join("etc/xp-ops/cloudflare_tunnel/api_token");
    fs::create_dir_all(token_path.parent().unwrap()).unwrap();
    fs::write(&token_path, "oldtoken").unwrap();

    let values = AppValues {
        node_name: String::new(),
        access_host: String::new(),
        cloudflare_enabled: true,
        ddns_enabled: false,
        ddns_zone_id: None,
        account_id: None,
        zone_id: None,
        hostname: None,
        origin_url: None,
        api_base_url: None,
        vless_canary_acme_contact_email: None,
        default_vless_port: None,
        default_vless_server_names: None,
        default_vless_fingerprint: None,
        default_ss_port: None,
        xray_version: "latest".to_string(),
        cloudflare_token: String::new(),
        enable_services: true,
        dry_run: false,
    };
    save_token_if_needed(&paths, &values).unwrap();

    let raw = fs::read_to_string(token_path).unwrap();
    assert_eq!(raw, "oldtoken");
}

#[test]
fn save_token_non_empty_writes_trimmed_value() {
    let (tmp, paths) = test_paths();
    let token_path = tmp.path().join("etc/xp-ops/cloudflare_tunnel/api_token");
    fs::create_dir_all(token_path.parent().unwrap()).unwrap();
    fs::write(&token_path, "oldtoken").unwrap();

    let values = AppValues {
        node_name: String::new(),
        access_host: String::new(),
        cloudflare_enabled: true,
        ddns_enabled: false,
        ddns_zone_id: None,
        account_id: None,
        zone_id: None,
        hostname: None,
        origin_url: None,
        api_base_url: None,
        vless_canary_acme_contact_email: None,
        default_vless_port: None,
        default_vless_server_names: None,
        default_vless_fingerprint: None,
        default_ss_port: None,
        xray_version: "latest".to_string(),
        cloudflare_token: " newtoken \n".to_string(),
        enable_services: true,
        dry_run: false,
    };
    save_token_if_needed(&paths, &values).unwrap();

    let raw = fs::read_to_string(token_path).unwrap();
    assert_eq!(raw, "newtoken");
}

#[test]
fn save_tui_config_error_includes_deploy_dir() {
    let (tmp, paths) = test_paths();
    let deploy_dir = tmp.path().join("etc/xp-ops/deploy");
    fs::create_dir_all(deploy_dir.parent().unwrap()).unwrap();
    fs::write(&deploy_dir, "not a dir").unwrap();

    let values = AppValues {
        node_name: String::new(),
        access_host: String::new(),
        cloudflare_enabled: true,
        ddns_enabled: false,
        ddns_zone_id: None,
        account_id: None,
        zone_id: None,
        hostname: None,
        origin_url: None,
        api_base_url: None,
        vless_canary_acme_contact_email: None,
        default_vless_port: None,
        default_vless_server_names: None,
        default_vless_fingerprint: None,
        default_ss_port: None,
        xray_version: "latest".to_string(),
        cloudflare_token: String::new(),
        enable_services: true,
        dry_run: false,
    };
    let err = save_tui_config(&paths, &values).unwrap_err();
    assert_eq!(err.code, 4);
    assert!(err.message.contains("ensure dir"));
    assert!(err.message.contains("etc/xp-ops/deploy"));
}

#[test]
fn save_token_error_includes_token_dir() {
    let (tmp, paths) = test_paths();
    let token_dir = tmp.path().join("etc/xp-ops/cloudflare_tunnel");
    fs::create_dir_all(token_dir.parent().unwrap()).unwrap();
    fs::write(&token_dir, "not a dir").unwrap();

    let values = AppValues {
        node_name: String::new(),
        access_host: String::new(),
        cloudflare_enabled: true,
        ddns_enabled: false,
        ddns_zone_id: None,
        account_id: None,
        zone_id: None,
        hostname: None,
        origin_url: None,
        api_base_url: None,
        vless_canary_acme_contact_email: None,
        default_vless_port: None,
        default_vless_server_names: None,
        default_vless_fingerprint: None,
        default_ss_port: None,
        xray_version: "latest".to_string(),
        cloudflare_token: "tok".to_string(),
        enable_services: true,
        dry_run: false,
    };
    let err = save_token_if_needed(&paths, &values).unwrap_err();
    assert_eq!(err.code, 4);
    assert!(err.message.contains("ensure dir"));
    assert!(err.message.contains("etc/xp-ops/cloudflare_tunnel"));
}
