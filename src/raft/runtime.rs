use std::sync::Arc;

use anyhow::Context;

use crate::{
    raft::{
        app::RealRaft,
        network_http::HttpNetworkFactory,
        storage::{FileLogStore, FileStateMachine},
        types::{NodeId, TypeConfig},
    },
    reconcile::ReconcileHandle,
    state::JsonSnapshotStore,
};

pub async fn start_raft(
    data_dir: &std::path::Path,
    cluster_name: String,
    node_id: NodeId,
    store: Arc<tokio::sync::Mutex<JsonSnapshotStore>>,
    reconcile: ReconcileHandle,
    network: HttpNetworkFactory,
) -> anyhow::Result<RealRaft> {
    let config = {
        #[cfg(test)]
        {
            openraft::Config {
                cluster_name,
                ..Default::default()
            }
        }

        #[cfg(not(test))]
        {
            // Production defaults: tuned for WAN-ish latencies (Cloudflare tunnels, etc.).
            // OpenRaft uses `heartbeat_interval` as the hard TTL for replication RPCs, so 50ms
            // is far too aggressive outside local networks.
            openraft::Config {
                cluster_name,
                heartbeat_interval: 2_000,
                election_timeout_min: 6_000,
                election_timeout_max: 12_000,
                install_snapshot_timeout: 30_000,
                ..Default::default()
            }
        }
    }
    .validate()
    .map_err(|e| anyhow::anyhow!("raft config validate: {e}"))?;

    let config = Arc::new(config);

    let log_store = FileLogStore::open(data_dir, node_id)
        .await
        .map_err(|e| anyhow::anyhow!("open log store: {e}"))?;
    let state_machine = FileStateMachine::open(data_dir, store, reconcile)
        .await
        .map_err(|e| anyhow::anyhow!("open state machine: {e}"))?;

    let raft =
        openraft::Raft::<TypeConfig>::new(node_id, config, network, log_store, state_machine)
            .await
            .context("start raft")?;

    let raft = RealRaft::new(raft);

    // NOTE: initialization is handled by the caller because it depends on cluster bootstrap mode.
    Ok(raft)
}
