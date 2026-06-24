#[cfg(xp_missing_web_dist)]
compile_error!("missing web/dist/index.html; run `cd web && bun run build`");

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use rustls::crypto::aws_lc_rs;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

use tokio::sync::{Mutex, watch};
use tokio::time::{Duration, Instant};

fn reject_legacy_relay_probe_env() -> Result<()> {
    if xp::ops::process_env_has_legacy_relay_probe_vars() {
        anyhow::bail!(xp::ops::LEGACY_RELAY_PROBE_REMOVED_MESSAGE);
    }
    Ok(())
}

fn install_rustls_crypto_provider() {
    let _ = aws_lc_rs::default_provider().install_default();
}

fn disable_managed_vless_reconcile_for_canary_result(
    vless_enabled: bool,
    canary_result: &anyhow::Result<Option<std::thread::JoinHandle<()>>>,
) -> bool {
    vless_enabled && matches!(canary_result, Ok(None) | Err(_))
}

fn should_reconcile_managed_defaults_at_startup(
    intent: &xp::managed_default_endpoints::ManagedDefaultEndpointsIntent,
) -> bool {
    !matches!(
        intent.vless,
        xp::managed_default_endpoints::ManagedDefaultEndpointIntent::Skip
    ) || !matches!(
        intent.ss,
        xp::managed_default_endpoints::ManagedDefaultEndpointIntent::Skip
    )
}

async fn reconcile_managed_defaults_with_startup_retries(
    data_dir: PathBuf,
    node_id: String,
    startup_endpoints: Vec<xp::domain::Endpoint>,
    intent: xp::managed_default_endpoints::ManagedDefaultEndpointsIntent,
    raft_facade: Arc<dyn xp::raft::app::RaftFacade>,
) {
    const ATTEMPTS: usize = 8;

    for attempt in 1..=ATTEMPTS {
        let mut writer = |cmd| async { raft_facade.client_write(cmd).await.map(|_| ()) };
        match xp::managed_default_endpoints::reconcile_managed_default_endpoints(
            &data_dir,
            &node_id,
            &startup_endpoints,
            &intent,
            &mut writer,
            "xp startup",
        )
        .await
        {
            Ok(()) => {
                if attempt > 1 {
                    tracing::info!(
                        attempt,
                        "managed default endpoint reconcile succeeded after startup retry"
                    );
                }
                return;
            }
            Err(err) if attempt < ATTEMPTS => {
                tracing::warn!(
                    attempt,
                    max_attempts = ATTEMPTS,
                    error = %err,
                    "managed default endpoint reconcile failed after startup; retrying"
                );
                tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
            }
            Err(err) => {
                tracing::error!(
                    attempt,
                    max_attempts = ATTEMPTS,
                    error = %err,
                    "managed default endpoint reconcile failed after startup retries"
                );
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    install_rustls_crypto_provider();
    reject_legacy_relay_probe_env()?;

    let cli = xp::config::Cli::parse();
    let cmd = cli.command.clone().unwrap_or(xp::config::Command::Run);
    let config = cli.config.clone();

    match cmd {
        xp::config::Command::Run => run_server(config).await,
        xp::config::Command::Init => init_cluster(&config),
        xp::config::Command::Join(args) => join_cluster(config, args.token).await,
        xp::config::Command::LoginLink => login_link(&config),
    }
}

fn init_cluster(config: &xp::config::Config) -> Result<()> {
    let meta = xp::cluster_metadata::ClusterMetadata::init_new_cluster(
        &config.data_dir,
        config.node_name.clone(),
        config.access_host.clone(),
        config.api_base_url.clone(),
    )?;

    let store = xp::state::JsonSnapshotStore::load_or_init(xp::state::StoreInit {
        data_dir: config.data_dir.clone(),
        bootstrap_node_id: Some(meta.node_id.clone()),
        bootstrap_node_name: meta.node_name.clone(),
        bootstrap_access_host: meta.access_host.clone(),
        bootstrap_api_base_url: meta.api_base_url.clone(),
    })?;

    if store.state().nodes.len() != 1 || !store.state().nodes.contains_key(&meta.node_id) {
        anyhow::bail!(
            "state.json already exists and is not compatible with newly initialized cluster metadata"
        );
    }

    Ok(())
}

fn login_link(config: &xp::config::Config) -> Result<()> {
    if config.admin_token_hash().is_none() {
        anyhow::bail!("admin token hash is not configured (XP_ADMIN_TOKEN_HASH is empty/invalid)");
    }
    if !config.api_base_url.starts_with("https://") {
        anyhow::bail!("--api-base-url must start with https://");
    }

    let cluster = xp::cluster_metadata::ClusterMetadata::load(&config.data_dir)?;
    let token_id = xp::id::new_ulid_string();
    let now = chrono::Utc::now();
    let jwt = xp::login_token::issue_login_token_jwt(
        &cluster.cluster_id,
        &token_id,
        now,
        &config.admin_token_hash,
    );

    let base = config.api_base_url.trim_end_matches('/');
    println!("{base}/login?login_token={jwt}");
    Ok(())
}

async fn join_cluster(config: xp::config::Config, join_token: String) -> Result<()> {
    use xp::cluster_identity::JoinToken;

    let token = JoinToken::decode_and_validate(&join_token, chrono::Utc::now())
        .map_err(|e| anyhow::anyhow!("decode join token: {e}"))?;
    let expected_node_id = token.token_id.clone();

    let csr = xp::cluster_identity::generate_node_keypair_and_csr(&expected_node_id)?;

    let url = format!(
        "{}/api/cluster/join",
        token.leader_api_base_url.trim_end_matches('/')
    );
    let cluster_ca = reqwest::Certificate::from_pem(token.cluster_ca_pem.as_bytes())?;
    let client = reqwest::Client::builder()
        .add_root_certificate(cluster_ca)
        .build()?;

    let node_name = config.node_name.clone();
    let access_host = config.access_host.clone();
    let api_base_url = config.api_base_url.clone();

    let req = serde_json::json!({
        "join_token": join_token,
        "node_name": node_name,
        "access_host": access_host,
        "api_base_url": api_base_url,
        "csr_pem": csr.csr_pem,
    });

    let resp = client
        .post(url)
        .json(&req)
        .send()
        .await?
        .error_for_status()?
        .json::<serde_json::Value>()
        .await?;

    let node_id = resp
        .get("node_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing node_id in join response"))?
        .to_string();
    let signed_cert_pem = resp
        .get("signed_cert_pem")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing signed_cert_pem in join response"))?
        .to_string();
    let cluster_ca_pem = resp
        .get("cluster_ca_pem")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing cluster_ca_pem in join response"))?
        .to_string();
    let cluster_ca_key_pem = resp
        .get("cluster_ca_key_pem")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing cluster_ca_key_pem in join response"))?
        .to_string();
    let xp_admin_token_hash = resp
        .get("xp_admin_token_hash")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing xp_admin_token_hash in join response"))?
        .to_string();

    if node_id != expected_node_id {
        anyhow::bail!(
            "leader returned unexpected node_id: expected {expected_node_id}, got {node_id}"
        );
    }

    let paths = xp::cluster_metadata::ClusterPaths::new(&config.data_dir);
    std::fs::create_dir_all(&paths.dir)?;
    std::fs::write(&paths.cluster_ca_pem, cluster_ca_pem.as_bytes())?;
    std::fs::write(&paths.cluster_ca_key_pem, cluster_ca_key_pem.as_bytes())?;
    best_effort_chmod_0600(&paths.cluster_ca_key_pem);
    std::fs::write(&paths.admin_token_hash, xp_admin_token_hash.as_bytes())?;
    best_effort_chmod_0600(&paths.admin_token_hash);
    std::fs::write(&paths.node_key_pem, csr.key_pem.as_bytes())?;
    std::fs::write(&paths.node_csr_pem, csr.csr_pem.as_bytes())?;
    std::fs::write(&paths.node_cert_pem, signed_cert_pem.as_bytes())?;

    let meta = xp::cluster_metadata::ClusterMetadata {
        schema_version: xp::cluster_metadata::CLUSTER_METADATA_SCHEMA_VERSION,
        cluster_id: token.cluster_id,
        node_id: node_id.clone(),
        node_name,
        access_host,
        api_base_url,
        has_cluster_ca_key: true,
        is_bootstrap_node: Some(false),
    };
    meta.save(&config.data_dir)?;

    let _store = xp::state::JsonSnapshotStore::load_or_init(xp::state::StoreInit {
        data_dir: config.data_dir.clone(),
        bootstrap_node_id: Some(node_id),
        bootstrap_node_name: meta.node_name.clone(),
        bootstrap_access_host: meta.access_host.clone(),
        bootstrap_api_base_url: meta.api_base_url.clone(),
    })?;

    Ok(())
}

async fn run_server(config: xp::config::Config) -> Result<()> {
    let cluster = xp::cluster_metadata::ClusterMetadata::load(&config.data_dir)?;
    let cluster_ca_pem = cluster.read_cluster_ca_pem(&config.data_dir)?;
    let cluster_ca_key_pem_required = cluster
        .read_cluster_ca_key_pem(&config.data_dir)?
        .ok_or_else(|| anyhow::anyhow!("cluster ca key is not available on this node"))?;
    let node_cert_pem = cluster.read_node_cert_pem(&config.data_dir)?;
    let node_key_pem = cluster.read_node_key_pem(&config.data_dir)?;

    let config_arc = Arc::new(config.clone());
    let mesh_proxy_state = if config.mesh_proxy_url.is_some() {
        xp::control_plane_mesh::MeshProxyStateHandle::ready()
    } else {
        xp::control_plane_mesh::MeshProxyStateHandle::disabled()
    };
    let store = xp::state::JsonSnapshotStore::load_or_init(xp::state::StoreInit {
        data_dir: config.data_dir.clone(),
        bootstrap_node_id: Some(cluster.node_id.clone()),
        bootstrap_node_name: cluster.node_name.clone(),
        bootstrap_access_host: cluster.access_host.clone(),
        bootstrap_api_base_url: cluster.api_base_url.clone(),
    })?;
    let store = Arc::new(Mutex::new(store));

    let reconcile = xp::reconcile::spawn_reconciler(
        config_arc.clone(),
        store.clone(),
        cluster_ca_key_pem_required.clone(),
    );
    let (xray_health, _xray_supervisor_task) =
        xp::xray_supervisor::spawn_xray_supervisor(config_arc.clone(), reconcile.clone());
    let (cloudflared_health, _cloudflared_supervisor_task) =
        xp::cloudflared_supervisor::spawn_cloudflared_supervisor(config_arc.clone());
    let (ddns_health, _ddns_supervisor_task) =
        xp::ddns::spawn_ddns_supervisor(config_arc.clone(), cloudflared_health.clone());
    let (node_runtime, _node_runtime_task) = xp::node_runtime::spawn_node_runtime_monitor(
        config_arc.clone(),
        cluster.node_id.clone(),
        xray_health.clone(),
        cloudflared_health.clone(),
        ddns_health,
    );
    let node_history = xp::node_history::NodeHistoryHandle::from_config(&config);
    let _node_history_local_task = xp::node_history::spawn_node_history_local_worker(
        config_arc.clone(),
        cluster.node_id.clone(),
        store.clone(),
        node_runtime.clone(),
        node_history.clone(),
    );

    let raft_id = xp::raft::types::raft_node_id_from_ulid(&cluster.node_id)?;
    let raft_network = xp::raft::network_http::HttpNetworkFactory::try_new_mtls_with_state(
        &cluster_ca_pem,
        &node_cert_pem,
        &node_key_pem,
        config.mesh_proxy_url.as_deref(),
        mesh_proxy_state.clone(),
    )?;
    let raft = xp::raft::runtime::start_raft(
        &config.data_dir,
        cluster.cluster_id.clone(),
        raft_id,
        store.clone(),
        reconcile.clone(),
        raft_network,
    )
    .await?;

    let raft_node_meta = xp::raft::types::NodeMeta {
        name: cluster.node_name.clone(),
        api_base_url: cluster.api_base_url.clone(),
        raft_endpoint: cluster.api_base_url.clone(),
    };

    if cluster.should_bootstrap_raft() {
        let was_initialized = raft
            .raft()
            .is_initialized()
            .await
            .map_err(|e| anyhow::anyhow!("raft is_initialized: {e}"))?;
        raft.initialize_single_node_if_needed(raft_id, raft_node_meta)
            .await?;
        if !was_initialized {
            // Ensure the bootstrap node exists in the Raft state machine so future joiners can
            // replicate the full node list. Without this, a joiner would only ever see itself
            // unless the leader later emits an explicit UpsertNode for the bootstrap node.
            let node = xp::domain::Node {
                node_id: cluster.node_id.clone(),
                node_name: cluster.node_name.clone(),
                access_host: cluster.access_host.clone(),
                api_base_url: cluster.api_base_url.clone(),
                quota_limit_bytes: 0,
                quota_reset: xp::domain::NodeQuotaReset::default(),
            };
            bootstrap_upsert_node(raft.raft(), node).await?;
        }
    }

    let explicit_managed_default_spec =
        xp::managed_default_endpoints::ManagedDefaultEndpointsSpec {
            vless: xp::managed_default_endpoints::build_default_vless_endpoint_spec(
                config.default_vless_port,
                &cluster.access_host,
                config.default_vless_server_names.as_deref(),
                config.default_vless_fingerprint.as_deref(),
                config.vless_canary_bind,
            )?,
            ss: xp::managed_default_endpoints::build_default_ss_endpoint_spec(
                config.default_ss_port,
            )?,
        };
    let endpoints = {
        let store = store.lock().await;
        store
            .list_endpoints()
            .into_iter()
            .filter(|endpoint| endpoint.node_id == cluster.node_id)
            .collect::<Vec<_>>()
    };
    let managed_default_state =
        xp::managed_default_endpoints::load_managed_default_endpoints_state(&config.data_dir)
            .context("load managed default endpoint state")?;
    let mut managed_default_intent =
        xp::managed_default_endpoints::resolve_host_managed_default_endpoints_intent(
            &explicit_managed_default_spec,
            &endpoints,
            &cluster.access_host,
            config.vless_canary_bind,
            &managed_default_state,
        )?;
    let vless_https_canary_task = if matches!(
        managed_default_intent.vless,
        xp::managed_default_endpoints::ManagedDefaultEndpointIntent::Manage { .. }
    ) {
        let canary_result = xp::vless_https_canary::spawn(
            config_arc.clone(),
            store.clone(),
            cluster.node_id.clone(),
        )
        .await;
        if disable_managed_vless_reconcile_for_canary_result(
            matches!(
                managed_default_intent.vless,
                xp::managed_default_endpoints::ManagedDefaultEndpointIntent::Manage { .. }
            ),
            &canary_result,
        ) {
            match &canary_result {
                Ok(None) => {
                    tracing::warn!(
                        "vless https canary is disabled; skipping managed default VLESS reconcile"
                    );
                }
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        "vless https canary preparation failed; skipping managed default VLESS reconcile"
                    );
                }
                Ok(Some(_)) => {}
            }
            managed_default_intent.vless =
                xp::managed_default_endpoints::ManagedDefaultEndpointIntent::Skip;
        }
        match canary_result {
            Ok(Some(handle)) => Some(handle),
            Ok(None) | Err(_) => None,
        }
    } else {
        xp::vless_https_canary::persist_disabled_status(&config.data_dir, config.vless_canary_bind)?;
        None
    };

    let raft_facade: Arc<dyn xp::raft::app::RaftFacade> =
        Arc::new(xp::raft::app::ForwardingRaftFacade::try_new(
            raft.raft(),
            cluster_ca_key_pem_required.clone(),
            &cluster_ca_pem,
            Some(&node_cert_pem),
            Some(&node_key_pem),
        )?);
    let pending_managed_default_reconcile = should_reconcile_managed_defaults_at_startup(
        &managed_default_intent,
    )
    .then_some((endpoints, managed_default_intent));
    let startup_raft_facade = raft_facade.clone();
    let startup_node_id = cluster.node_id.clone();
    let (geo_db_update, _geo_db_update_task) =
        xp::ip_geo_db::spawn_geo_db_update_worker(config_arc.clone(), store.clone())?;
    let _quota = xp::quota::spawn_quota_worker(
        config_arc.clone(),
        store.clone(),
        reconcile.clone(),
        geo_db_update.resolver(),
    );
    let _node_history_remote_sync_task = xp::node_history::spawn_node_history_remote_sync_worker(
        cluster.node_id.clone(),
        store.clone(),
        node_history.clone(),
        cluster_ca_pem.clone(),
        cluster_ca_key_pem_required.clone(),
    );

    let probe_secret = cluster_ca_key_pem_required.clone();
    let endpoint_probe = xp::endpoint_probe::spawn_endpoint_probe_worker(
        cluster.node_id.clone(),
        store.clone(),
        raft_facade.clone(),
        probe_secret,
        config.endpoint_probe_skip_self_test,
    );
    let (node_egress_probe, _node_egress_probe_task) =
        xp::node_egress_probe::spawn_node_egress_probe_worker(
            config_arc.clone(),
            cluster.node_id.clone(),
            store.clone(),
            raft_facade.clone(),
        )?;
    let _vless_https_canary_task = vless_https_canary_task;

    let app = xp::http::build_router(
        config.clone(),
        store.clone(),
        reconcile,
        xray_health,
        cloudflared_health,
        node_runtime,
        node_history,
        endpoint_probe,
        node_egress_probe,
        cluster,
        cluster_ca_pem,
        Some(cluster_ca_key_pem_required),
        raft_facade,
        Some(raft.raft()),
        geo_db_update,
        mesh_proxy_state,
    )
    .layer(TraceLayer::new_for_http())
    .layer(CorsLayer::permissive());

    info!(
        bind = %config.bind,
        data_dir = %config.data_dir.display(),
        "starting xp"
    );
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    if let Some((startup_endpoints, startup_managed_default_intent)) =
        pending_managed_default_reconcile
    {
        let data_dir = config.data_dir.clone();
        let raft_facade = startup_raft_facade;
        let node_id = startup_node_id;
        tokio::spawn(async move {
            reconcile_managed_defaults_with_startup_retries(
                data_dir,
                node_id,
                startup_endpoints,
                startup_managed_default_intent,
                raft_facade,
            )
            .await;
        });
    }
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(filter).compact().init();
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

async fn wait_for_raft_leader(
    mut metrics: watch::Receiver<
        openraft::RaftMetrics<xp::raft::types::NodeId, xp::raft::types::NodeMeta>,
    >,
    timeout: Duration,
) -> anyhow::Result<()> {
    let started_at = Instant::now();
    loop {
        let snapshot = metrics.borrow().clone();
        if matches!(snapshot.state, openraft::ServerState::Leader) {
            return Ok(());
        }
        let elapsed = Instant::now().duration_since(started_at);
        if elapsed >= timeout {
            anyhow::bail!(
                "timeout waiting for raft leader: state={:?}",
                snapshot.state
            );
        }
        let remaining = timeout - elapsed;
        tokio::time::timeout(remaining, metrics.changed())
            .await
            .map_err(|_| anyhow::anyhow!("timeout waiting for raft leader"))?
            .map_err(|e| anyhow::anyhow!("raft metrics channel closed: {e}"))?;
    }
}

async fn bootstrap_upsert_node(
    raft: openraft::Raft<xp::raft::types::TypeConfig>,
    node: xp::domain::Node,
) -> anyhow::Result<()> {
    wait_for_raft_leader(raft.metrics(), Duration::from_secs(30)).await?;

    let mut backoff = Duration::from_millis(100);
    for attempt in 0..5u8 {
        match raft
            .client_write(xp::state::DesiredStateCommand::UpsertNode { node: node.clone() })
            .await
        {
            Ok(_resp) => return Ok(()),
            Err(err) => {
                if let Some(openraft::error::ClientWriteError::ForwardToLeader(_)) = err.api_error()
                {
                    tracing::warn!(
                        attempt,
                        backoff_ms = backoff.as_millis(),
                        "bootstrap upsert_node forwarded; retrying"
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = std::cmp::min(backoff * 2, Duration::from_secs(2));
                    continue;
                }
                return Err(anyhow::anyhow!("bootstrap raft upsert_node: {err}"));
            }
        }
    }

    anyhow::bail!("bootstrap raft upsert_node: leader not available after retries");
}

fn best_effort_chmod_0600(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{CertificateParams, DnType, KeyPair, PKCS_ECDSA_P256_SHA256};
    use rustls::crypto::CryptoProvider;
    use time::OffsetDateTime;

    #[tokio::test]
    async fn installs_rustls_provider_before_tls_setup() {
        install_rustls_crypto_provider();
        assert!(CryptoProvider::get_default().is_some());

        let mut params = CertificateParams::new(vec!["canary.example.com".to_string()]).unwrap();
        params
            .distinguished_name
            .push(DnType::CommonName, "canary.example.com");
        params.not_before = OffsetDateTime::now_utc() - time::Duration::days(1);
        params.not_after = OffsetDateTime::now_utc() + time::Duration::days(30);

        let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
        let cert = params.self_signed(&key).unwrap();
        let _ = axum_server::tls_rustls::RustlsConfig::from_pem(
            cert.pem().into_bytes(),
            key.serialize_pem().into_bytes(),
        )
        .await
        .unwrap();
    }

    #[test]
    fn reject_legacy_relay_probe_env_fails_when_old_vars_exist() {
        let key = "XP_RELAY_PROBE_BIND";
        let original = std::env::var_os(key);
        unsafe { std::env::set_var(key, "127.0.0.1:443") };

        let result = super::reject_legacy_relay_probe_env();

        match original {
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }

        let err = result.expect_err("legacy relay-probe env must be rejected");
        assert!(
            err.to_string().contains("XP_RELAY_PROBE_* has been removed"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn disable_managed_vless_reconcile_when_canary_is_disabled() {
        let result: anyhow::Result<Option<std::thread::JoinHandle<()>>> = Ok(None);
        assert!(super::disable_managed_vless_reconcile_for_canary_result(
            true, &result
        ));
    }

    #[test]
    fn keep_managed_vless_reconcile_when_canary_handle_exists() {
        let handle = std::thread::spawn(|| {});
        let result: anyhow::Result<Option<std::thread::JoinHandle<()>>> = Ok(Some(handle));
        assert!(!super::disable_managed_vless_reconcile_for_canary_result(
            true, &result
        ));
        let Ok(Some(handle)) = result else {
            panic!("test fixture should keep the canary handle");
        };
        handle.join().unwrap();
    }

    #[test]
    fn do_not_disable_when_vless_is_not_managed() {
        let result: anyhow::Result<Option<std::thread::JoinHandle<()>>> = Ok(None);
        assert!(!super::disable_managed_vless_reconcile_for_canary_result(
            false, &result
        ));
    }

    #[test]
    fn startup_reconcile_runs_when_any_managed_default_intent_is_not_skip() {
        let manage_vless = xp::managed_default_endpoints::ManagedDefaultEndpointsIntent {
            vless: xp::managed_default_endpoints::ManagedDefaultEndpointIntent::Manage {
                spec: xp::managed_default_endpoints::DefaultVlessEndpointSpec {
                    port: 53844,
                    reality_dest: "127.0.0.1:39043".to_string(),
                    server_names: vec!["example.com".to_string()],
                    server_names_source:
                        xp::protocol::RealityServerNamesSource::Manual,
                    fingerprint: "chrome".to_string(),
                },
                source: xp::managed_default_endpoints::ManagedDefaultEndpointSource::Explicit,
            },
            ss: xp::managed_default_endpoints::ManagedDefaultEndpointIntent::Skip,
        };
        assert!(super::should_reconcile_managed_defaults_at_startup(
            &manage_vless
        ));

        let manage_ss = xp::managed_default_endpoints::ManagedDefaultEndpointsIntent {
            vless: xp::managed_default_endpoints::ManagedDefaultEndpointIntent::Skip,
            ss: xp::managed_default_endpoints::ManagedDefaultEndpointIntent::Manage {
                spec: xp::managed_default_endpoints::DefaultSsEndpointSpec { port: 53845 },
                source: xp::managed_default_endpoints::ManagedDefaultEndpointSource::Explicit,
            },
        };
        assert!(super::should_reconcile_managed_defaults_at_startup(
            &manage_ss
        ));
    }

    #[test]
    fn startup_reconcile_skips_when_all_managed_default_intents_are_skip() {
        let intent = xp::managed_default_endpoints::ManagedDefaultEndpointsIntent {
            vless: xp::managed_default_endpoints::ManagedDefaultEndpointIntent::Skip,
            ss: xp::managed_default_endpoints::ManagedDefaultEndpointIntent::Skip,
        };
        assert!(!super::should_reconcile_managed_defaults_at_startup(
            &intent
        ));
    }
}
