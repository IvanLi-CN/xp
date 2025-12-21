use anyhow::Result;
use std::sync::Arc;

use clap::Parser;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let cli = xp::config::Cli::parse();
    let cmd = cli.command.clone().unwrap_or(xp::config::Command::Run);

    match cmd {
        xp::config::Command::Run => run_server(cli.config).await,
        xp::config::Command::Init => init_cluster(&cli.config),
        xp::config::Command::Join(args) => join_cluster(cli.config, args.token).await,
    }
}

fn init_cluster(config: &xp::config::Config) -> Result<()> {
    let meta = xp::cluster_metadata::ClusterMetadata::init_new_cluster(
        &config.data_dir,
        config.node_name.clone(),
        config.public_domain.clone(),
        config.api_base_url.clone(),
    )?;

    let store = xp::state::JsonSnapshotStore::load_or_init(xp::state::StoreInit {
        data_dir: config.data_dir.clone(),
        bootstrap_node_id: Some(meta.node_id.clone()),
        bootstrap_node_name: meta.node_name.clone(),
        bootstrap_public_domain: meta.public_domain.clone(),
        bootstrap_api_base_url: meta.api_base_url.clone(),
    })?;

    if store.state().nodes.len() != 1 || !store.state().nodes.contains_key(&meta.node_id) {
        anyhow::bail!(
            "state.json already exists and is not compatible with newly initialized cluster metadata"
        );
    }

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
        .tls_built_in_root_certs(false)
        .add_root_certificate(cluster_ca)
        .build()?;

    let node_name = config.node_name.clone();
    let public_domain = config.public_domain.clone();
    let api_base_url = config.api_base_url.clone();

    let req = serde_json::json!({
        "join_token": join_token,
        "node_name": node_name,
        "public_domain": public_domain,
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
    std::fs::write(&paths.node_key_pem, csr.key_pem.as_bytes())?;
    std::fs::write(&paths.node_csr_pem, csr.csr_pem.as_bytes())?;
    std::fs::write(&paths.node_cert_pem, signed_cert_pem.as_bytes())?;

    let meta = xp::cluster_metadata::ClusterMetadata {
        schema_version: xp::cluster_metadata::CLUSTER_METADATA_SCHEMA_VERSION,
        cluster_id: token.cluster_id,
        node_id: node_id.clone(),
        node_name,
        public_domain,
        api_base_url,
        has_cluster_ca_key: true,
        is_bootstrap_node: Some(false),
    };
    meta.save(&config.data_dir)?;

    let _store = xp::state::JsonSnapshotStore::load_or_init(xp::state::StoreInit {
        data_dir: config.data_dir.clone(),
        bootstrap_node_id: Some(node_id),
        bootstrap_node_name: meta.node_name.clone(),
        bootstrap_public_domain: meta.public_domain.clone(),
        bootstrap_api_base_url: meta.api_base_url.clone(),
    })?;

    Ok(())
}

async fn run_server(mut config: xp::config::Config) -> Result<()> {
    let cluster = xp::cluster_metadata::ClusterMetadata::load(&config.data_dir)?;
    let cluster_ca_pem = cluster.read_cluster_ca_pem(&config.data_dir)?;
    let cluster_ca_key_pem = cluster.read_cluster_ca_key_pem(&config.data_dir)?;
    let node_cert_pem = cluster.read_node_cert_pem(&config.data_dir)?;
    let node_key_pem = cluster.read_node_key_pem(&config.data_dir)?;

    // Prefer persisted cluster metadata for node identity fields.
    config.node_name = cluster.node_name.clone();
    config.public_domain = cluster.public_domain.clone();
    config.api_base_url = cluster.api_base_url.clone();

    let config_arc = Arc::new(config.clone());
    let store = xp::state::JsonSnapshotStore::load_or_init(xp::state::StoreInit {
        data_dir: config.data_dir.clone(),
        bootstrap_node_id: Some(cluster.node_id.clone()),
        bootstrap_node_name: cluster.node_name.clone(),
        bootstrap_public_domain: cluster.public_domain.clone(),
        bootstrap_api_base_url: cluster.api_base_url.clone(),
    })?;
    let store = Arc::new(Mutex::new(store));

    let reconcile = xp::reconcile::spawn_reconciler(config_arc.clone(), store.clone());
    let _quota =
        xp::quota::spawn_quota_worker(config_arc.clone(), store.clone(), reconcile.clone());

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
        raft.initialize_single_node_if_needed(raft_id, raft_node_meta)
            .await?;
    }

    let raft_facade: Arc<dyn xp::raft::app::RaftFacade> =
        Arc::new(xp::raft::app::ForwardingRaftFacade::try_new(
            raft.raft(),
            config.admin_token.clone(),
            &cluster_ca_pem,
            Some(&node_cert_pem),
            Some(&node_key_pem),
        )?);

    let app = xp::http::build_router(
        config.clone(),
        store.clone(),
        reconcile,
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

fn best_effort_chmod_0600(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
}
