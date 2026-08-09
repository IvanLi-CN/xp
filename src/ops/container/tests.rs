use super::*;
use crate::managed_default_endpoints::{
    build_managed_default_vless_endpoint, reconcile_managed_default_vless_endpoint,
};
use crate::protocol::VlessRealityVisionTcpEndpointMeta;
use tempfile::tempdir;
use tokio::process::Command;

const VALID_ADMIN_TOKEN_HASH: &str = concat!(
    "$argon2id$v=19$m=4096,t=3,p=1$TqOws+M/ypxKCmnVcbWAdg$",
    "QdZvInnh6DNxvD4ZfwAGd/C/eR43+tT7eBPaPcqVjFM"
);
const HIGH_MEMORY_ADMIN_TOKEN_HASH: &str = concat!(
    "$argon2id$v=19$m=65536,t=3,p=1$TqOws+M/ypxKCmnVcbWAdg$",
    "VlLbEUvXvoESmlktijJp9QYD/jJklIIljA1vuce9P+k"
);

fn env_map(values: &[(&str, &str)]) -> BTreeMap<String, String> {
    values
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

#[tokio::test]
async fn cutover_marker_command_writes_to_the_selected_data_dir() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    cmd_mark_internal_auth_v2_cutover(
        paths,
        InternalAuthV2CutoverMarkerArgs {
            data_dir: PathBuf::from("/var/lib/xp/data"),
            dry_run: false,
        },
    )
    .await
    .unwrap();

    assert!(
        tmp.path()
            .join("var/lib/xp/data/mesh/internal-auth-v2-cutover.json")
            .exists()
    );
}

#[tokio::test]
async fn cutover_marker_can_be_cancelled_before_the_epoch_is_consumed() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let args = InternalAuthV2CutoverMarkerArgs {
        data_dir: PathBuf::from("/var/lib/xp/data"),
        dry_run: false,
    };
    cmd_mark_internal_auth_v2_cutover(paths.clone(), args.clone())
        .await
        .unwrap();
    cmd_cancel_internal_auth_v2_cutover(paths, args)
        .await
        .unwrap();

    assert!(
        !tmp.path()
            .join("var/lib/xp/data/mesh/internal-auth-v2-cutover.json")
            .exists()
    );
}

#[tokio::test]
async fn bootstrap_requires_admin_token() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let env = env_map(&[
        ("XP_NODE_NAME", "node-1"),
        ("XP_API_BASE_URL", "https://node-1.example.com"),
    ]);
    let err = ContainerSpec::from_env_map(&paths, &env, None)
        .await
        .unwrap_err();
    assert!(
        err.message
            .contains("bootstrap mode requires XP_ADMIN_TOKEN or XP_ADMIN_TOKEN_HASH")
    );
}

#[tokio::test]
async fn bootstrap_derives_access_host_from_api_base_url() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let env = env_map(&[
        ("XP_NODE_NAME", "node-1"),
        ("XP_API_BASE_URL", "https://node-1.example.com"),
        ("XP_ADMIN_TOKEN", "0123456789abcdef0123456789abcdef"),
    ]);
    let spec = ContainerSpec::from_env_map(&paths, &env, None)
        .await
        .unwrap();
    assert_eq!(spec.access_host, "node-1.example.com");
    assert!(matches!(
        spec.startup,
        ContainerStartup::Bootstrap { needs_init: true }
    ));
    assert!(spec.configured_admin_token_hash.is_some());
}

#[tokio::test]
async fn managed_vless_uses_derived_access_host_when_access_host_env_is_absent() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let runtime_token = paths.map_abs(Path::new(crate::config::DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE));
    fs::create_dir_all(runtime_token.parent().unwrap()).unwrap();
    fs::write(&runtime_token, "runtime-token\n").unwrap();
    let env = env_map(&[
        ("XP_NODE_NAME", "node-1"),
        ("XP_API_BASE_URL", "https://node-1.example.com"),
        ("XP_ADMIN_TOKEN", "0123456789abcdef0123456789abcdef"),
        ("XP_DEFAULT_VLESS_PORT", "53842"),
    ]);
    let spec = ContainerSpec::from_env_map(&paths, &env, None)
        .await
        .unwrap();

    assert_eq!(spec.access_host, "node-1.example.com");
    assert_eq!(
        spec.default_endpoints.vless.as_ref().unwrap().server_names,
        vec!["node-1.example.com"]
    );
}

#[test]
fn cloudflare_api_base_url_defaults_to_hostname() {
    let cf = ContainerCloudflare {
        account_id: "acc".to_string(),
        zone_id: "zone".to_string(),
        zone_name: "example.com".to_string(),
        hostname: xp_test_fixtures::slot_s556().to_owned(),
        tunnel_name: "xp-node-1".to_string(),
        origin_url: DEFAULT_CLOUDFLARE_ORIGIN_URL.to_string(),
        token: xp_test_fixtures::slot_s460().to_owned(),
        token_source: CloudflareTokenSource::Env,
    };
    let env = env_map(&[]);
    let api_base_url = resolve_api_base_url(&env, Some(&cf)).unwrap();
    let access_host = resolve_access_host(&env, &api_base_url, Some(&cf)).unwrap();
    assert_eq!(
        api_base_url,
        format!("https://{}", xp_test_fixtures::slot_s556())
    );
    assert_eq!(access_host, xp_test_fixtures::slot_s556());
}

#[test]
fn decodes_join_token_leader_api_base_url() {
    let token = crate::cluster_identity::JoinToken {
        cluster_id: xp_test_fixtures::slot_s557().to_owned(),
        leader_api_base_url: xp_test_fixtures::slot_s558().to_owned(),
        cluster_ca_pem: "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----\n".to_string(),
        token_id: "token-id".to_string(),
        one_time_secret: "secret".to_string(),
        expires_at: chrono::Utc::now(),
    }
    .encode_base64url_json();
    assert_eq!(
        decode_join_token_leader_api_base_url(&token).as_deref(),
        Some(xp_test_fixtures::slot_s558())
    );
}

#[tokio::test]
async fn existing_container_metadata_ignores_stale_join_token_leader() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let token = crate::cluster_identity::JoinToken {
        cluster_id: xp_test_fixtures::slot_s557().to_owned(),
        leader_api_base_url: xp_test_fixtures::slot_s559().to_owned(),
        cluster_ca_pem: "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----\n".to_string(),
        token_id: xp_test_fixtures::slot_s560().to_owned(),
        one_time_secret: "secret".to_string(),
        expires_at: chrono::Utc::now(),
    }
    .encode_base64url_json();
    let meta = ClusterMetadata {
        schema_version: crate::cluster_metadata::CLUSTER_METADATA_SCHEMA_VERSION,
        cluster_id: xp_test_fixtures::slot_s557().to_owned(),
        node_id: xp_test_fixtures::slot_s560().to_owned(),
        node_name: xp_test_fixtures::slot_s605().to_owned(),
        access_host: xp_test_fixtures::slot_s556().to_owned(),
        api_base_url: xp_test_fixtures::slot_s561().to_owned(),
        has_cluster_ca_key: true,
        is_bootstrap_node: Some(false),
    };
    let env = env_map(&[
        ("XP_NODE_NAME", "node-1"),
        ("XP_API_BASE_URL", "https://node-1.example.com"),
        ("XP_JOIN_TOKEN", &token),
    ]);

    let spec = ContainerSpec::from_env_map(&paths, &env, Some(&meta))
        .await
        .unwrap();

    assert!(spec.join_leader_api_base_url.is_none());
}

#[tokio::test]
async fn joined_container_syncs_configured_low_memory_admin_token_hash() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let data_dir = paths.map_abs(Path::new(DEFAULT_DATA_DIR));
    let cluster_paths = ClusterPaths::new(&data_dir);
    fs::create_dir_all(&cluster_paths.dir).unwrap();
    fs::write(
        &cluster_paths.admin_token_hash,
        HIGH_MEMORY_ADMIN_TOKEN_HASH,
    )
    .unwrap();
    let meta = ClusterMetadata {
        schema_version: crate::cluster_metadata::CLUSTER_METADATA_SCHEMA_VERSION,
        cluster_id: xp_test_fixtures::slot_s557().to_owned(),
        node_id: xp_test_fixtures::slot_s562().to_owned(),
        node_name: xp_test_fixtures::slot_s605().to_owned(),
        access_host: xp_test_fixtures::slot_s556().to_owned(),
        api_base_url: xp_test_fixtures::slot_s561().to_owned(),
        has_cluster_ca_key: true,
        is_bootstrap_node: Some(false),
    };
    let env = env_map(&[
        ("XP_NODE_NAME", "node-1"),
        ("XP_API_BASE_URL", "https://node-1.example.com"),
        ("XP_ADMIN_TOKEN_HASH", VALID_ADMIN_TOKEN_HASH),
    ]);

    let spec = ContainerSpec::from_env_map(&paths, &env, Some(&meta))
        .await
        .unwrap();
    reconcile_configured_admin_token_hash(&paths, &spec).unwrap();

    assert_eq!(
        fs::read_to_string(&cluster_paths.admin_token_hash).unwrap(),
        format!("{VALID_ADMIN_TOKEN_HASH}\n")
    );
    assert_eq!(
        effective_admin_token_hash(&paths, &spec).unwrap(),
        VALID_ADMIN_TOKEN_HASH
    );
}

#[tokio::test]
async fn first_join_preserves_leader_admin_token_hash() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let data_dir = paths.map_abs(Path::new(DEFAULT_DATA_DIR));
    let cluster_paths = ClusterPaths::new(&data_dir);
    fs::create_dir_all(&cluster_paths.dir).unwrap();
    fs::write(
        &cluster_paths.admin_token_hash,
        HIGH_MEMORY_ADMIN_TOKEN_HASH,
    )
    .unwrap();
    let env = env_map(&[
        ("XP_NODE_NAME", "node-1"),
        ("XP_API_BASE_URL", "https://node-1.example.com"),
        ("XP_JOIN_TOKEN", "first-join-token"),
        ("XP_ADMIN_TOKEN_HASH", VALID_ADMIN_TOKEN_HASH),
    ]);

    let spec = ContainerSpec::from_env_map(&paths, &env, None)
        .await
        .unwrap();
    reconcile_configured_admin_token_hash(&paths, &spec).unwrap();

    assert_eq!(
        fs::read_to_string(&cluster_paths.admin_token_hash).unwrap(),
        HIGH_MEMORY_ADMIN_TOKEN_HASH
    );
}

#[tokio::test]
async fn joined_container_rejects_high_memory_configured_admin_token_hash() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let meta = ClusterMetadata {
        schema_version: crate::cluster_metadata::CLUSTER_METADATA_SCHEMA_VERSION,
        cluster_id: xp_test_fixtures::slot_s557().to_owned(),
        node_id: xp_test_fixtures::slot_s562().to_owned(),
        node_name: xp_test_fixtures::slot_s605().to_owned(),
        access_host: xp_test_fixtures::slot_s556().to_owned(),
        api_base_url: xp_test_fixtures::slot_s561().to_owned(),
        has_cluster_ca_key: true,
        is_bootstrap_node: Some(false),
    };
    let env = env_map(&[
        ("XP_NODE_NAME", "node-1"),
        ("XP_API_BASE_URL", "https://node-1.example.com"),
        ("XP_ADMIN_TOKEN_HASH", HIGH_MEMORY_ADMIN_TOKEN_HASH),
    ]);

    let err = ContainerSpec::from_env_map(&paths, &env, Some(&meta))
        .await
        .unwrap_err();

    assert!(err.message.contains("m=4096,t=3,p=1"));
}

#[tokio::test]
async fn joined_container_without_configured_hash_preserves_persisted_hash() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let data_dir = paths.map_abs(Path::new(DEFAULT_DATA_DIR));
    let cluster_paths = ClusterPaths::new(&data_dir);
    fs::create_dir_all(&cluster_paths.dir).unwrap();
    fs::write(
        &cluster_paths.admin_token_hash,
        HIGH_MEMORY_ADMIN_TOKEN_HASH,
    )
    .unwrap();
    let meta = ClusterMetadata {
        schema_version: crate::cluster_metadata::CLUSTER_METADATA_SCHEMA_VERSION,
        cluster_id: xp_test_fixtures::slot_s557().to_owned(),
        node_id: xp_test_fixtures::slot_s562().to_owned(),
        node_name: xp_test_fixtures::slot_s605().to_owned(),
        access_host: xp_test_fixtures::slot_s556().to_owned(),
        api_base_url: xp_test_fixtures::slot_s561().to_owned(),
        has_cluster_ca_key: true,
        is_bootstrap_node: Some(false),
    };
    let env = env_map(&[
        ("XP_NODE_NAME", "node-1"),
        ("XP_API_BASE_URL", "https://node-1.example.com"),
    ]);

    let spec = ContainerSpec::from_env_map(&paths, &env, Some(&meta))
        .await
        .unwrap();
    reconcile_configured_admin_token_hash(&paths, &spec).unwrap();

    assert_eq!(
        fs::read_to_string(&cluster_paths.admin_token_hash).unwrap(),
        HIGH_MEMORY_ADMIN_TOKEN_HASH
    );
}

#[test]
fn zone_candidates_walk_suffixes() {
    assert_eq!(
        zone_name_candidates("a.b.example.com"),
        vec!["a.b.example.com", "b.example.com", "example.com", "com"]
    );
}

#[test]
fn detects_metadata_mismatch_without_blocking_reuse() {
    let meta = ClusterMetadata {
        schema_version: crate::cluster_metadata::CLUSTER_METADATA_SCHEMA_VERSION,
        cluster_id: xp_test_fixtures::slot_s557().to_owned(),
        node_id: xp_test_fixtures::slot_s562().to_owned(),
        node_name: xp_test_fixtures::slot_s605().to_owned(),
        access_host: xp_test_fixtures::slot_s556().to_owned(),
        api_base_url: xp_test_fixtures::slot_s561().to_owned(),
        has_cluster_ca_key: true,
        is_bootstrap_node: Some(true),
    };
    assert!(node_meta_mismatch(
        &meta,
        "node-2",
        "node-2.example.com",
        "https://node-2.example.com",
    ));
    let startup = resolve_startup(Some(&meta), None).unwrap();
    assert!(matches!(
        startup,
        ContainerStartup::Bootstrap { needs_init: false }
    ));
}

#[test]
fn parses_default_endpoint_specs_from_env() {
    let env = env_map(&[
        ("XP_ACCESS_HOST", "node-1-ep.example.com"),
        ("XP_DEFAULT_VLESS_PORT", "53842"),
        (
            "XP_DEFAULT_VLESS_SERVER_NAMES",
            "cdn-a.example.test, cdn-b.example.test",
        ),
        ("XP_DEFAULT_SS_PORT", "53843"),
    ]);
    let spec = ManagedDefaultEndpointsSpec::from_env_map(&env, "node-1-ep.example.com").unwrap();
    assert_eq!(spec.vless.as_ref().unwrap().port, 53842);
    assert_eq!(
        spec.vless.as_ref().unwrap().reality_dest,
        crate::config::DEFAULT_VLESS_CANARY_BIND
    );
    assert_eq!(
        spec.vless.as_ref().unwrap().server_names,
        vec!["node-1-ep.example.com"]
    );
    assert_eq!(spec.vless.as_ref().unwrap().fingerprint, "chrome");
    assert_eq!(spec.ss.as_ref().unwrap().port, 53843);
}

#[test]
fn auxiliary_vless_env_does_not_require_a_bootstrap_port() {
    let env = env_map(&[
        ("XP_DEFAULT_VLESS_SERVER_NAMES", "cdn-a.example.test"),
        ("XP_DEFAULT_VLESS_FINGERPRINT", "chrome"),
    ]);

    let spec = ManagedDefaultEndpointsSpec::from_env_map(&env, "node-1-ep.example.com").unwrap();

    assert!(spec.vless.is_none());
}

#[test]
fn default_vless_canary_bind_must_be_socket_addr() {
    let env = env_map(&[
        ("XP_ACCESS_HOST", "node-1-ep.example.com"),
        ("XP_DEFAULT_VLESS_PORT", "53842"),
        ("XP_DEFAULT_VLESS_SERVER_NAMES", "cdn-a.example.test"),
        ("XP_VLESS_CANARY_BIND", "bad-bind"),
    ]);

    let err = ManagedDefaultEndpointsSpec::from_env_map(&env, "node-1-ep.example.com").unwrap_err();
    assert!(err.message.contains("XP_VLESS_CANARY_BIND"));
}

#[test]
fn default_vless_canary_bind_can_be_overridden() {
    let env = env_map(&[
        ("XP_ACCESS_HOST", "node-1-ep.example.com"),
        ("XP_DEFAULT_VLESS_PORT", "53842"),
        ("XP_DEFAULT_VLESS_SERVER_NAMES", "cdn-a.example.test"),
        ("XP_VLESS_CANARY_BIND", "127.0.0.1:49043"),
    ]);

    let spec = ManagedDefaultEndpointsSpec::from_env_map(&env, "node-1-ep.example.com").unwrap();
    assert_eq!(spec.vless.as_ref().unwrap().reality_dest, "127.0.0.1:49043");
}

#[tokio::test]
async fn container_runtime_env_includes_vless_canary_settings() {
    let env = env_map(&[
        ("XP_NODE_NAME", "node-1"),
        ("XP_API_BASE_URL", "https://node-1.example.com"),
        ("XP_ACCESS_HOST", "node-1.example.com"),
        ("XP_ADMIN_TOKEN_HASH", VALID_ADMIN_TOKEN_HASH),
        ("XP_VLESS_CANARY_BIND", "127.0.0.1:49043"),
        (
            "XP_VLESS_CANARY_ACME_DIRECTORY_URL",
            "https://acme-staging-v02.api.letsencrypt.org/directory",
        ),
        ("XP_VLESS_CANARY_ACME_CONTACT_EMAIL", "ops@example.com"),
        ("XP_VLESS_CANARY_CLOUDFLARE_TOKEN_FILE", "/custom/token"),
        ("XP_VLESS_CANARY_CLOUDFLARE_ZONE_ID", "zone-123"),
        ("XP_DEFAULT_VLESS_PORT", "53842"),
        ("XP_DEFAULT_VLESS_SERVER_NAMES", "cdn-a.example.test"),
    ]);
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    fs::create_dir_all(paths.etc_xp_ops_cloudflare_dir()).unwrap();
    fs::write(paths.etc_xp_ops_cloudflare_token(), "cloudflare-token").unwrap();
    let spec = ContainerSpec::from_env_map(&paths, &env, None)
        .await
        .unwrap();
    assert_eq!(
        spec.runtime_env
            .get("XP_VLESS_CANARY_BIND")
            .map(String::as_str),
        Some("127.0.0.1:49043")
    );
    assert_eq!(
        spec.runtime_env
            .get("XP_VLESS_CANARY_ACME_DIRECTORY_URL")
            .map(String::as_str),
        Some("https://acme-staging-v02.api.letsencrypt.org/directory")
    );
    assert_eq!(
        spec.runtime_env
            .get("XP_VLESS_CANARY_ACME_CONTACT_EMAIL")
            .map(String::as_str),
        Some("ops@example.com")
    );
    assert_eq!(
        spec.runtime_env
            .get("XP_VLESS_CANARY_CLOUDFLARE_TOKEN_FILE")
            .map(String::as_str),
        Some("/custom/token")
    );
    assert_eq!(
        spec.runtime_env
            .get("XP_VLESS_CANARY_CLOUDFLARE_ZONE_ID")
            .map(String::as_str),
        Some("zone-123")
    );
}

#[test]
fn build_runtime_env_forwards_vless_canary_and_default_endpoint_settings() {
    let env = env_map(&[
        ("XP_VLESS_CANARY_BIND", "127.0.0.1:49043"),
        (
            "XP_VLESS_CANARY_ACME_DIRECTORY_URL",
            "https://acme-staging-v02.api.letsencrypt.org/directory",
        ),
        ("XP_VLESS_CANARY_ACME_CONTACT_EMAIL", "ops@example.com"),
        ("XP_VLESS_CANARY_CLOUDFLARE_TOKEN_FILE", "/custom/token"),
        ("XP_VLESS_CANARY_CLOUDFLARE_ZONE_ID", "zone-123"),
        ("XP_DEFAULT_VLESS_PORT", "53842"),
        ("XP_DEFAULT_VLESS_SERVER_NAMES", "cdn-a.example.test"),
        ("XP_DEFAULT_VLESS_FINGERPRINT", "firefox"),
        ("XP_DEFAULT_SS_PORT", "53843"),
    ]);

    let runtime_env = build_runtime_env(&env, None);
    assert_eq!(
        runtime_env.get("XP_VLESS_CANARY_BIND").map(String::as_str),
        Some("127.0.0.1:49043")
    );
    assert_eq!(
        runtime_env
            .get("XP_VLESS_CANARY_ACME_DIRECTORY_URL")
            .map(String::as_str),
        Some("https://acme-staging-v02.api.letsencrypt.org/directory")
    );
    assert_eq!(
        runtime_env
            .get("XP_VLESS_CANARY_ACME_CONTACT_EMAIL")
            .map(String::as_str),
        Some("ops@example.com")
    );
    assert_eq!(
        runtime_env
            .get("XP_VLESS_CANARY_CLOUDFLARE_TOKEN_FILE")
            .map(String::as_str),
        Some("/custom/token")
    );
    assert_eq!(
        runtime_env
            .get("XP_VLESS_CANARY_CLOUDFLARE_ZONE_ID")
            .map(String::as_str),
        Some("zone-123")
    );
    assert_eq!(
        runtime_env.get("XP_DEFAULT_VLESS_PORT").map(String::as_str),
        Some("53842")
    );
    assert_eq!(
        runtime_env
            .get("XP_DEFAULT_VLESS_SERVER_NAMES")
            .map(String::as_str),
        Some("cdn-a.example.test")
    );
    assert_eq!(
        runtime_env
            .get("XP_DEFAULT_VLESS_FINGERPRINT")
            .map(String::as_str),
        Some("firefox")
    );
    assert_eq!(
        runtime_env.get("XP_DEFAULT_SS_PORT").map(String::as_str),
        Some("53843")
    );
    assert_eq!(
        runtime_env
            .get("XP_CLOUDFLARED_GOMEMLIMIT")
            .map(String::as_str),
        Some("12MiB")
    );
    assert_eq!(
        runtime_env
            .get("XP_CLOUDFLARED_MANAGEMENT_DIAGNOSTICS")
            .map(String::as_str),
        Some("false")
    );
    assert_eq!(
        runtime_env
            .get("XP_CLOUDFLARED_PROTOCOL")
            .map(String::as_str),
        Some("http2")
    );
}

#[test]
fn cloudflared_protocol_defaults_to_http2_and_allows_an_override() {
    assert_eq!(cloudflared_protocol(&BTreeMap::new()), "http2");
    let runtime_env = BTreeMap::from([("XP_CLOUDFLARED_PROTOCOL".to_string(), "quic".to_string())]);
    assert_eq!(cloudflared_protocol(&runtime_env), "quic");
}

#[test]
fn prepare_runtime_inputs_honors_custom_canary_token_file() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let spec = ContainerSpec {
        node_name: xp_test_fixtures::slot_s605().to_owned(),
        access_host: xp_test_fixtures::slot_s556().to_owned(),
        api_base_url: xp_test_fixtures::slot_s561().to_owned(),
        data_dir: PathBuf::from("/var/lib/xp/data"),
        bind: xp_test_fixtures::slot_s563().parse().unwrap(),
        xray_api_addr: "127.0.0.1:10085".parse().unwrap(),
        startup: ContainerStartup::Bootstrap { needs_init: true },
        configured_admin_token_hash: Some(VALID_ADMIN_TOKEN_HASH.to_string()),
        cloudflare: None,
        ddns: None,
        vless_canary_token: Some("custom-token".to_string()),
        node_meta_needs_realign: false,
        default_endpoints: ManagedDefaultEndpointsSpec::default(),
        join_leader_api_base_url: None,
        runtime_env: BTreeMap::from([(
            "XP_VLESS_CANARY_CLOUDFLARE_TOKEN_FILE".to_string(),
            "/custom/token".to_string(),
        )]),
    };

    prepare_runtime_inputs(&paths, &spec, None, Mode::Real).unwrap();

    let written = fs::read_to_string(paths.map_abs(Path::new("/custom/token"))).unwrap();
    assert_eq!(written, "custom-token\n");
}

#[tokio::test]
async fn container_spec_loads_canary_token_from_configured_runtime_path() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let runtime_token = paths.map_abs(Path::new("/custom/canary-token"));
    fs::create_dir_all(runtime_token.parent().unwrap()).unwrap();
    fs::write(&runtime_token, "runtime-token\n").unwrap();

    let env = env_map(&[
        ("XP_NODE_NAME", "node-1"),
        ("XP_API_BASE_URL", "https://node-1.example.com"),
        ("XP_ACCESS_HOST", "node-1.example.com"),
        ("XP_ADMIN_TOKEN_HASH", VALID_ADMIN_TOKEN_HASH),
        ("XP_DEFAULT_VLESS_PORT", "53842"),
        ("XP_DEFAULT_VLESS_SERVER_NAMES", "cdn-a.example.test"),
        (
            "XP_VLESS_CANARY_CLOUDFLARE_TOKEN_FILE",
            "/custom/canary-token",
        ),
    ]);

    let spec = ContainerSpec::from_env_map(&paths, &env, None)
        .await
        .unwrap();
    assert_eq!(spec.vless_canary_token.as_deref(), Some("runtime-token"));
}

#[tokio::test]
async fn container_spec_loads_canary_token_from_default_runtime_path() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let runtime_token = paths.map_abs(Path::new(crate::config::DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE));
    fs::create_dir_all(runtime_token.parent().unwrap()).unwrap();
    fs::write(&runtime_token, "default-runtime-token\n").unwrap();

    let env = env_map(&[
        ("XP_NODE_NAME", "node-1"),
        ("XP_API_BASE_URL", "https://node-1.example.com"),
        ("XP_ACCESS_HOST", "node-1.example.com"),
        ("XP_ADMIN_TOKEN_HASH", VALID_ADMIN_TOKEN_HASH),
        ("XP_DEFAULT_VLESS_PORT", "53842"),
        ("XP_DEFAULT_VLESS_SERVER_NAMES", "cdn-a.example.test"),
    ]);

    let spec = ContainerSpec::from_env_map(&paths, &env, None)
        .await
        .unwrap();
    assert_eq!(
        spec.vless_canary_token.as_deref(),
        Some("default-runtime-token")
    );
}

#[test]
fn vless_reconcile_preserves_keys_and_updates_reality_settings() {
    let current = DefaultVlessEndpointSpec {
        port: 53842,
        reality_dest: crate::config::DEFAULT_VLESS_CANARY_BIND.to_string(),
        server_names: xp_test_fixtures::slot_l36(),
        server_names_source: crate::protocol::RealityServerNamesSource::Manual,
        fingerprint: "chrome".to_string(),
    };
    let endpoint = build_managed_default_vless_endpoint(&current, "node-id".to_string()).unwrap();

    let desired = DefaultVlessEndpointSpec {
        port: 60000,
        reality_dest: "127.0.0.1:49043".to_string(),
        server_names: xp_test_fixtures::slot_l36(),
        server_names_source: crate::protocol::RealityServerNamesSource::Manual,
        fingerprint: "firefox".to_string(),
    };
    let updated = reconcile_managed_default_vless_endpoint(&desired, &endpoint).unwrap();
    let old_meta: VlessRealityVisionTcpEndpointMeta =
        serde_json::from_value(endpoint.meta.clone()).unwrap();
    let new_meta: VlessRealityVisionTcpEndpointMeta =
        serde_json::from_value(updated.meta.clone()).unwrap();
    assert_eq!(updated.port, 53842);
    assert_eq!(new_meta.reality.dest, "127.0.0.1:49043");
    assert_eq!(new_meta.reality.server_names, xp_test_fixtures::slot_l36());
    assert_eq!(new_meta.reality.fingerprint, "firefox");
    assert_eq!(new_meta.reality_keys, old_meta.reality_keys);
    assert_eq!(new_meta.short_ids, old_meta.short_ids);
    assert_eq!(new_meta.active_short_id, old_meta.active_short_id);
}

#[tokio::test]
async fn child_startup_wait_accepts_long_running_process() {
    let mut child = Command::new("sh").args(["-c", "sleep 1"]).spawn().unwrap();
    wait_for_child_startup("test-child", &mut child, Duration::from_millis(50))
        .await
        .unwrap();
    cleanup_child(&mut child).await;
}

#[tokio::test]
async fn child_startup_wait_rejects_early_exit() {
    let mut child = Command::new("sh").args(["-c", "exit 23"]).spawn().unwrap();
    let err = wait_for_child_startup("test-child", &mut child, Duration::from_secs(1))
        .await
        .unwrap_err();
    assert!(
        err.message
            .contains("container_failed: test-child exited with code 23")
    );
}
