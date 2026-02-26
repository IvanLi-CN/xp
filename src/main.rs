use anyhow::Result;
use std::sync::Arc;

use clap::Parser;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

use tokio::sync::{Mutex, watch};
use tokio::time::{Duration, Instant};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

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
    let cluster_ca_key_pem = cluster.read_cluster_ca_key_pem(&config.data_dir)?;
    let node_cert_pem = cluster.read_node_cert_pem(&config.data_dir)?;
    let node_key_pem = cluster.read_node_key_pem(&config.data_dir)?;

    let config_arc = Arc::new(config.clone());
    let store = xp::state::JsonSnapshotStore::load_or_init(xp::state::StoreInit {
        data_dir: config.data_dir.clone(),
        bootstrap_node_id: Some(cluster.node_id.clone()),
        bootstrap_node_name: cluster.node_name.clone(),
        bootstrap_access_host: cluster.access_host.clone(),
        bootstrap_api_base_url: cluster.api_base_url.clone(),
    })?;
    let store = Arc::new(Mutex::new(store));

    let reconcile = xp::reconcile::spawn_reconciler(config_arc.clone(), store.clone());
    let (xray_health, _xray_supervisor_task) =
        xp::xray_supervisor::spawn_xray_supervisor(config_arc.clone(), reconcile.clone());
    let (cloudflared_health, _cloudflared_supervisor_task) =
        xp::cloudflared_supervisor::spawn_cloudflared_supervisor(config_arc.clone());
    let (node_runtime, _node_runtime_task) = xp::node_runtime::spawn_node_runtime_monitor(
        config_arc.clone(),
        cluster.node_id.clone(),
        xray_health.clone(),
        cloudflared_health,
    );

    let raft_id = xp::raft::types::raft_node_id_from_ulid(&cluster.node_id)?;
    let raft_network = xp::raft::network_http::HttpNetworkFactory::try_new_mtls(
        &cluster_ca_pem,
        &node_cert_pem,
        &node_key_pem,
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
                quota_reset: xp::domain::NodeQuotaReset::default(),
            };
            bootstrap_upsert_node(raft.raft(), node).await?;
        }
    }

    let raft_facade: Arc<dyn xp::raft::app::RaftFacade> =
        Arc::new(xp::raft::app::ForwardingRaftFacade::try_new(
            raft.raft(),
            cluster_ca_key_pem
                .clone()
                .ok_or_else(|| anyhow::anyhow!("cluster ca key is not available on this node"))?,
            &cluster_ca_pem,
            Some(&node_cert_pem),
            Some(&node_key_pem),
        )?);
    let _quota = xp::quota::spawn_quota_worker(
        config_arc.clone(),
        store.clone(),
        reconcile.clone(),
        raft_facade.clone(),
    );

    let probe_secret = cluster_ca_key_pem
        .clone()
        .ok_or_else(|| anyhow::anyhow!("cluster ca key is not available on this node"))?;
    let endpoint_probe = xp::endpoint_probe::spawn_endpoint_probe_worker(
        cluster.node_id.clone(),
        store.clone(),
        raft_facade.clone(),
        probe_secret,
        config.endpoint_probe_skip_self_test,
    );

    let app = xp::http::build_router(
        config.clone(),
        store.clone(),
        reconcile,
        xray_health,
        node_runtime,
        endpoint_probe,
        cluster,
        cluster_ca_pem,
        cluster_ca_key_pem,
        raft_facade,
        Some(raft.raft()),
    )
    .layer(TraceLayer::new_for_http())
    .layer(CorsLayer::permissive());

    info!(
        bind = %config.bind,
        data_dir = %config.data_dir.display(),
        "starting xp"
    );
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
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
