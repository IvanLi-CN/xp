use std::{env, path::PathBuf, sync::Arc};

use tokio::{process::Command, task::JoinHandle};
use tracing::{error, info};

use crate::config::Config;

/// Starts the independent `xp-proxy` process only when the explicit capability is enabled.
/// Snapshot publication is deliberately separate; this supervisor owns process lifecycle only.
pub fn spawn_native_proxy_supervisor(config: Arc<Config>) -> Option<JoinHandle<()>> {
    let enabled = env::var("XP_NATIVE_PROXY_ENABLED")
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes"));
    if !enabled {
        return None;
    }
    let _ = config;
    let binary = env::var("XP_NATIVE_PROXY_BIN").unwrap_or_else(|_| "xp-proxy".to_string());
    let socket = env::var_os("XP_NATIVE_PROXY_CONTROL_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/run/xp/xp-proxy.sock"));
    Some(tokio::spawn(async move {
        if let Err(error) = run(binary, socket).await {
            error!(%error, "native proxy supervisor stopped");
        }
    }))
}

async fn run(binary: String, socket: PathBuf) -> anyhow::Result<()> {
    if let Some(parent) = socket.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut child = Command::new(&binary)
        .arg("--control-socket")
        .arg(&socket)
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| anyhow::anyhow!("spawn {binary}: {error}"))?;
    info!(binary = %binary, socket = %socket.display(), "native proxy process started");
    let status = child.wait().await?;
    anyhow::bail!("native proxy exited with {status}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn disabled_capability_does_not_spawn_a_process() {
        let config = Arc::new(crate::config::Cli::try_parse_from(["xp"]).unwrap().config);
        assert!(spawn_native_proxy_supervisor(config).is_none());
    }
}
