use std::net::SocketAddr;

use anyhow::Result;
use axum::{Json, Router, routing::get};
use clap::Parser;
use serde::Serialize;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Parser, Debug)]
#[command(
    name = "xp",
    about = "Xray control plane",
    disable_help_subcommand = true
)]
struct CliArgs {
    #[arg(long, value_name = "ADDR", default_value = "127.0.0.1:62416")]
    bind: SocketAddr,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let cli = CliArgs::parse();

    let app = Router::new()
        .route("/api/health", get(health))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive());

    info!(bind = %cli.bind, "starting xp");
    let listener = tokio::net::TcpListener::bind(cli.bind).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(filter).compact().init();
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
