use anyhow::Result;
use std::sync::Arc;

use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

use clap::Parser;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let config = xp::config::Config::parse();
    let store = xp::state::JsonSnapshotStore::load_or_init(xp::state::StoreInit {
        data_dir: config.data_dir.clone(),
        bootstrap_node_name: config.node_name.clone(),
        bootstrap_public_domain: config.public_domain.clone(),
        bootstrap_api_base_url: config.api_base_url.clone(),
    })?;
    let store = Arc::new(Mutex::new(store));

    let app = xp::http::build_router(config.clone(), store)
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
