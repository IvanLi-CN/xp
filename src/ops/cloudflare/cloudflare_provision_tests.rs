use super::*;
use crate::ops::cli::CloudflareProvisionArgs;
use crate::ops::util::ENV_LOCK;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn response(result: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "success": true, "errors": [], "result": result })
}

fn migration_args(dry_run: bool) -> CloudflareProvisionArgs {
    CloudflareProvisionArgs {
        tunnel_name: Some("xp-next".to_string()),
        account_id: "account".to_string(),
        zone_id: "zone".to_string(),
        hostname: "xp.example.com".to_string(),
        origin_url: "http://127.0.0.1:62416".to_string(),
        dns_record_id_override: None,
        tunnel_id_override: None,
        migrate_existing_tunnel: true,
        enable: false,
        no_enable: true,
        dry_run,
    }
}

fn existing_target_migration_args(dry_run: bool) -> CloudflareProvisionArgs {
    let mut args = migration_args(dry_run);
    args.tunnel_id_override = Some("new".to_string());
    args.migrate_existing_tunnel = false;
    args
}

fn write_migration_files(paths: &Paths) {
    fs::create_dir_all(paths.etc_xp_ops_cloudflare_dir()).unwrap();
    fs::create_dir_all(paths.etc_cloudflared_dir()).unwrap();
    let cloudflared = cloudflared_binary_path(paths, Distro::Rhel);
    fs::create_dir_all(cloudflared.parent().unwrap()).unwrap();
    fs::write(&cloudflared, b"test").unwrap();
    #[cfg(unix)]
    fs::set_permissions(&cloudflared, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        paths.etc_xp_ops_cloudflare_settings(),
        serde_json::json!({
            "enabled": true,
            "install_mode": "external",
            "origin_url": "http://127.0.0.1:62416",
            "account_id": "account",
            "zone_id": "zone",
            "hostname": "xp.example.com",
            "tunnel_id": "old",
            "dns_record_id": "record"
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        paths.etc_cloudflared_dir().join("old.json"),
        r#"{"TunnelID":"old"}"#,
    )
    .unwrap();
}

#[test]
fn deferred_provision_can_mark_persisted_tunnel_enabled_after_service_start() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    write_migration_files(&paths);
    let settings_path = paths.etc_xp_ops_cloudflare_settings();
    let deferred = fs::read_to_string(&settings_path)
        .unwrap()
        .replace("\"enabled\": true", "\"enabled\": false");
    fs::write(&settings_path, deferred).unwrap();

    set_persisted_tunnel_enabled(&paths, true).unwrap();

    let settings = fs::read_to_string(settings_path).unwrap();
    assert!(settings.contains("\"enabled\": true"));
}

#[test]
fn marking_tunnel_enabled_requires_provisioned_settings() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());

    let error = set_persisted_tunnel_enabled(&paths, true)
        .expect_err("missing provisioned Tunnel settings must fail deploy");
    assert_eq!(error.code, 6);
    assert!(error.message.contains("cloudflare_settings_missing"));
}

async fn mount_old_tunnel_preflight(server: &MockServer, ingress: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path(
            "/client/v4/accounts/account/cfd_tunnel/old/configurations",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(response(
            serde_json::json!({ "config": { "ingress": ingress } }),
        )))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/client/v4/zones/zone/dns_records"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(response(serde_json::json!([{
                "id": "record",
                "type": "CNAME",
                "name": "xp.example.com",
                "content": "old.cfargotunnel.com",
                "proxied": true,
                "ttl": 1
            }]))),
        )
        .mount(server)
        .await;
}

async fn mount_existing_target_config(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path(
            "/client/v4/accounts/account/cfd_tunnel/new/configurations",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(response(
            serde_json::json!({ "config": { "ingress": [{ "service": "http_status:404" }] } }),
        )))
        .mount(server)
        .await;
}

#[test]
fn migration_requires_only_the_owned_hostname() {
    assert!(migration_owns_only_hostname(
        &["xp.example.com".to_string(), "xp.example.com".to_string()],
        "xp.example.com",
    ));
    assert!(!migration_owns_only_hostname(&[], "xp.example.com"));
    assert!(!migration_owns_only_hostname(
        &["xp.example.com".to_string(), "ssh.example.com".to_string()],
        "xp.example.com",
    ));
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn shared_legacy_tunnel_fails_before_any_cloudflare_write() {
    let server = MockServer::start().await;
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    write_migration_files(&paths);
    mount_old_tunnel_preflight(
        &server,
        serde_json::json!([
            { "hostname": "xp.example.com", "service": "http://old" },
            { "hostname": "ssh.example.com", "service": "ssh://localhost:22" },
            { "service": "http_status:404" }
        ]),
    )
    .await;

    let error = {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("CLOUDFLARE_API_BASE_URL", server.uri());
            std::env::set_var("XP_OPS_DISTRO", "rhel");
        }
        let result = run(
            paths,
            migration_args(false),
            "token".to_string(),
            ProvisionRuntime::ManagedService,
        )
        .await
        .unwrap_err();
        unsafe {
            std::env::remove_var("CLOUDFLARE_API_BASE_URL");
            std::env::remove_var("XP_OPS_DISTRO");
        }
        result
    };

    assert_eq!(error.code, 2);
    assert!(error.message.contains("shared Tunnel"));
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method.as_str(), "GET");
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn existing_target_migration_dry_run_uses_only_get_requests() {
    let server = MockServer::start().await;
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    write_migration_files(&paths);
    mount_old_tunnel_preflight(
        &server,
        serde_json::json!([
            { "hostname": "xp.example.com", "service": "http://old" },
            { "service": "http_status:404" }
        ]),
    )
    .await;
    mount_existing_target_config(&server).await;

    {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("CLOUDFLARE_API_BASE_URL", server.uri());
            std::env::set_var("XP_OPS_DISTRO", "rhel");
        }
        let result = run(
            paths,
            existing_target_migration_args(true),
            "token".to_string(),
            ProvisionRuntime::ManagedService,
        )
        .await;
        unsafe {
            std::env::remove_var("CLOUDFLARE_API_BASE_URL");
            std::env::remove_var("XP_OPS_DISTRO");
        }
        result.unwrap();
    }

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 3);
    assert!(
        requests
            .iter()
            .all(|request| request.method.as_str() == "GET")
    );
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn fresh_tunnel_dry_run_does_not_call_cloudflare() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    write_migration_files(&paths);
    fs::remove_file(paths.etc_xp_ops_cloudflare_settings()).unwrap();

    let _lock = ENV_LOCK.lock().unwrap();
    unsafe { std::env::set_var("XP_OPS_DISTRO", "rhel") };
    let result = run(
        paths,
        CloudflareProvisionArgs {
            migrate_existing_tunnel: false,
            ..migration_args(true)
        },
        "token".to_string(),
        ProvisionRuntime::ManagedService,
    )
    .await;
    unsafe { std::env::remove_var("XP_OPS_DISTRO") };

    result.unwrap();
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn single_hostname_legacy_tunnel_migrates_automatically() {
    let server = MockServer::start().await;
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    write_migration_files(&paths);
    mount_old_tunnel_preflight(
        &server,
        serde_json::json!([
            { "hostname": "xp.example.com", "path": "/old", "service": "http://old" },
            { "hostname": "xp.example.com", "service": "http://also-old" },
            { "service": "http_status:404" }
        ]),
    )
    .await;
    Mock::given(method("POST"))
        .and(path("/client/v4/accounts/account/cfd_tunnel"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response(
            serde_json::json!({ "id": "new", "credentials_file": { "TunnelID": "new" } }),
        )))
        .expect(1)
        .mount(&server)
        .await;
    mount_existing_target_config(&server).await;
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response(serde_json::json!({}))))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/client/v4/zones/zone/dns_records/record"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response(serde_json::json!({}))))
        .expect(1)
        .mount(&server)
        .await;

    {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("CLOUDFLARE_API_BASE_URL", server.uri());
            std::env::set_var("XP_OPS_DISTRO", "rhel");
        }
        let result = run(
            paths.clone(),
            migration_args(false),
            "token".to_string(),
            ProvisionRuntime::ManagedService,
        )
        .await;
        unsafe {
            std::env::remove_var("CLOUDFLARE_API_BASE_URL");
            std::env::remove_var("XP_OPS_DISTRO");
        }
        result.unwrap();
    }

    let settings = fs::read_to_string(paths.etc_xp_ops_cloudflare_settings()).unwrap();
    assert!(settings.contains("\"tunnel_id\": \"new\""));
    let requests = server.received_requests().await.unwrap();
    let methods: Vec<_> = requests
        .iter()
        .map(|request| request.method.as_str())
        .collect();
    assert_eq!(
        methods,
        ["GET", "GET", "POST", "GET", "PUT", "PATCH", "PUT"]
    );
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn existing_target_migration_runs_without_compatibility_flag() {
    let server = MockServer::start().await;
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    write_migration_files(&paths);
    fs::write(
        paths.etc_cloudflared_dir().join("new.json"),
        r#"{"TunnelID":"new"}"#,
    )
    .unwrap();
    mount_old_tunnel_preflight(
        &server,
        serde_json::json!([
            { "hostname": "xp.example.com", "service": "http://old" },
            { "service": "http_status:404" }
        ]),
    )
    .await;
    mount_existing_target_config(&server).await;
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response(serde_json::json!({}))))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/client/v4/zones/zone/dns_records/record"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response(serde_json::json!({}))))
        .expect(1)
        .mount(&server)
        .await;

    {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("CLOUDFLARE_API_BASE_URL", server.uri());
            std::env::set_var("XP_OPS_DISTRO", "rhel");
        }
        let result = run(
            paths.clone(),
            existing_target_migration_args(false),
            "token".to_string(),
            ProvisionRuntime::ManagedService,
        )
        .await;
        unsafe {
            std::env::remove_var("CLOUDFLARE_API_BASE_URL");
            std::env::remove_var("XP_OPS_DISTRO");
        }
        result.unwrap();
    }

    let settings = fs::read_to_string(paths.etc_xp_ops_cloudflare_settings()).unwrap();
    assert!(settings.contains("\"tunnel_id\": \"new\""));
    let requests = server.received_requests().await.unwrap();
    let methods: Vec<_> = requests
        .iter()
        .map(|request| request.method.as_str())
        .collect();
    assert_eq!(methods, ["GET", "GET", "GET", "PUT", "PATCH", "PUT"]);
}

#[test]
fn credentials_must_belong_to_the_requested_tunnel() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let credentials = paths.etc_cloudflared_dir().join("expected.json");
    fs::create_dir_all(credentials.parent().unwrap()).unwrap();
    fs::write(&credentials, r#"{"TunnelID":"different"}"#).unwrap();

    let error = verify_tunnel_credentials(&paths, "expected", "preflight").unwrap_err();

    assert_eq!(error.code, 6);
    assert!(error.message.contains("do not belong"));
}
