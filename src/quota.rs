use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::{
    config::Config,
    cycle::{CycleWindowError, current_cycle_window_at, effective_cycle_policy_and_day},
    raft::app::RaftFacade,
    reconcile::ReconcileHandle,
    state::{DesiredStateCommand, GrantEnabledSource, JsonSnapshotStore},
    xray,
};

const QUOTA_TOLERANCE_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct QuotaHandle {
    shutdown: Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

impl QuotaHandle {
    pub async fn shutdown(&self) {
        let tx = self.shutdown.lock().await.take();
        if let Some(tx) = tx {
            let _ = tx.send(());
        }
    }
}

pub fn spawn_quota_worker(
    config: Arc<Config>,
    store: Arc<Mutex<JsonSnapshotStore>>,
    reconcile: ReconcileHandle,
    raft: Arc<dyn RaftFacade>,
) -> QuotaHandle {
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let handle = QuotaHandle {
        shutdown: Arc::new(tokio::sync::Mutex::new(Some(shutdown_tx))),
    };

    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(Duration::from_secs(config.quota_poll_interval_secs));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let now = Utc::now();
                    if let Err(err) = run_quota_tick_at(now, &config, &store, &reconcile, &raft).await {
                        warn!(%err, "quota tick failed");
                    }
                }
                _ = &mut shutdown_rx => break,
            }
        }
    });

    handle
}

#[derive(Debug, Clone)]
struct GrantQuotaSnapshot {
    grant_id: String,
    endpoint_tag: Option<String>,
    quota_limit_bytes: u64,
    cycle_policy: crate::cycle::EffectiveCyclePolicy,
    cycle_day_of_month: u8,
    prev_cycle_start_at: Option<String>,
    prev_cycle_end_at: Option<String>,
}

fn map_cycle_error(grant_id: &str, err: CycleWindowError) -> anyhow::Error {
    anyhow::anyhow!("grant_id={grant_id} cycle window error: {err}")
}

async fn set_grant_enabled_via_raft(
    raft: &Arc<dyn RaftFacade>,
    grant_id: &str,
    enabled: bool,
) -> anyhow::Result<()> {
    let resp = raft
        .client_write(DesiredStateCommand::SetGrantEnabled {
            grant_id: grant_id.to_string(),
            enabled,
            source: GrantEnabledSource::Quota,
        })
        .await?;
    match resp {
        crate::raft::types::ClientResponse::Ok { .. } => Ok(()),
        crate::raft::types::ClientResponse::Err {
            status,
            code,
            message,
        } => Err(anyhow::anyhow!(
            "raft client_write failed: status={status} code={code} message={message}"
        )),
    }
}

pub async fn run_quota_tick_at(
    now: DateTime<Utc>,
    config: &Config,
    store: &Arc<Mutex<JsonSnapshotStore>>,
    reconcile: &ReconcileHandle,
    raft: &Arc<dyn RaftFacade>,
) -> anyhow::Result<()> {
    let snapshots = {
        let store = store.lock().await;
        let Some(local_node_id) = crate::reconcile::resolve_local_node_id(config, &store) else {
            warn!(
                node_name = %config.node_name,
                api_base_url = %config.api_base_url,
                "quota tick: local node_id not found; skipping xray calls"
            );
            return Ok(());
        };

        let endpoints_by_id = store
            .list_endpoints()
            .into_iter()
            .map(|e| (e.endpoint_id.clone(), e))
            .collect::<std::collections::BTreeMap<_, _>>();

        let mut out = Vec::new();
        for grant in store.list_grants() {
            let endpoint = endpoints_by_id.get(&grant.endpoint_id);
            if let Some(endpoint) = endpoint
                && endpoint.node_id != local_node_id
            {
                continue;
            }

            let (policy, day) = match effective_cycle_policy_and_day(&store, &grant) {
                Ok(v) => v,
                Err(err) => {
                    warn!(
                        grant_id = grant.grant_id,
                        %err,
                        "quota tick skip grant: cycle policy resolution failed"
                    );
                    continue;
                }
            };

            let usage = store.get_grant_usage(&grant.grant_id);
            out.push(GrantQuotaSnapshot {
                grant_id: grant.grant_id,
                endpoint_tag: endpoint.map(|e| e.tag.clone()),
                quota_limit_bytes: grant.quota_limit_bytes,
                cycle_policy: policy,
                cycle_day_of_month: day,
                prev_cycle_start_at: usage.as_ref().map(|u| u.cycle_start_at.clone()),
                prev_cycle_end_at: usage.as_ref().map(|u| u.cycle_end_at.clone()),
            });
        }
        out
    };

    if snapshots.is_empty() {
        return Ok(());
    }

    let mut client = match xray::connect(config.xray_api_addr).await {
        Ok(client) => client,
        Err(err) => {
            warn!(%err, "quota tick skip: xray connect failed");
            return Ok(());
        }
    };

    for snapshot in snapshots {
        if let Err(err) =
            process_grant_once(now, config, store, reconcile, raft, &mut client, snapshot).await
        {
            warn!(%err, "quota tick: grant processing failed");
        }
    }

    Ok(())
}

async fn process_grant_once(
    now: DateTime<Utc>,
    config: &Config,
    store: &Arc<Mutex<JsonSnapshotStore>>,
    reconcile: &ReconcileHandle,
    raft: &Arc<dyn RaftFacade>,
    client: &mut xray::XrayClient,
    snapshot: GrantQuotaSnapshot,
) -> anyhow::Result<()> {
    let (cycle_start, cycle_end) =
        current_cycle_window_at(snapshot.cycle_policy, snapshot.cycle_day_of_month, now)
            .map_err(|err| map_cycle_error(&snapshot.grant_id, err))?;
    let cycle_start_at = cycle_start.to_rfc3339();
    let cycle_end_at = cycle_end.to_rfc3339();

    let email = format!("grant:{}", snapshot.grant_id);
    let (uplink_total, downlink_total) = match client.get_user_traffic_totals(&email).await {
        Ok(v) => v,
        Err(status) => {
            warn!(
                grant_id = snapshot.grant_id,
                %status,
                "quota tick: xray get_user_traffic_totals failed"
            );
            return Ok(());
        }
    };

    let seen_at = now.to_rfc3339();

    let (used_bytes, window_changed, quota_banned, grant_enabled) = {
        let mut store = store.lock().await;
        let snapshot_usage = store.apply_grant_usage_sample(
            &snapshot.grant_id,
            cycle_start_at.clone(),
            cycle_end_at.clone(),
            uplink_total,
            downlink_total,
            seen_at,
        )?;

        let window_changed = match (
            snapshot.prev_cycle_start_at.as_deref(),
            snapshot.prev_cycle_end_at.as_deref(),
        ) {
            (Some(prev_start), Some(prev_end)) => {
                prev_start != cycle_start_at || prev_end != cycle_end_at
            }
            _ => true,
        };

        let usage_after = store.get_grant_usage(&snapshot.grant_id);
        let quota_banned = usage_after.as_ref().is_some_and(|u| u.quota_banned);
        let grant_enabled = store
            .get_grant(&snapshot.grant_id)
            .is_some_and(|g| g.enabled);

        (
            snapshot_usage.used_bytes,
            window_changed,
            quota_banned,
            grant_enabled,
        )
    };

    if window_changed && config.quota_auto_unban && quota_banned {
        debug!(
            grant_id = snapshot.grant_id,
            "quota tick: cycle rollover detected, auto-unbanning"
        );
        if grant_enabled {
            let mut store = store.lock().await;
            store.clear_quota_banned(&snapshot.grant_id)?;
            reconcile.request_full();
            return Ok(());
        }

        match set_grant_enabled_via_raft(raft, &snapshot.grant_id, true).await {
            Ok(_) => {
                let mut store = store.lock().await;
                store.clear_quota_banned(&snapshot.grant_id)?;
                reconcile.request_full();
            }
            Err(err) => {
                warn!(
                    grant_id = snapshot.grant_id,
                    %err,
                    "quota tick: auto-unban enable via raft failed"
                );
            }
        }
        return Ok(());
    }

    if quota_banned && grant_enabled {
        if let Err(err) = set_grant_enabled_via_raft(raft, &snapshot.grant_id, false).await {
            debug!(
                grant_id = snapshot.grant_id,
                %err,
                "quota tick: raft disable retry failed"
            );
        }
        return Ok(());
    }

    if snapshot.quota_limit_bytes == 0 {
        return Ok(());
    }

    let threshold_reached =
        used_bytes.saturating_add(QUOTA_TOLERANCE_BYTES) >= snapshot.quota_limit_bytes;
    if !threshold_reached || !grant_enabled {
        return Ok(());
    }

    {
        let mut store = store.lock().await;
        if !quota_banned {
            store.set_quota_banned(&snapshot.grant_id, now.to_rfc3339())?;
        }
    }
    reconcile.request_full();

    if let Some(tag) = snapshot.endpoint_tag.as_deref() {
        use crate::xray::proto::xray::app::proxyman::command::AlterInboundRequest;
        let op = crate::xray::builder::build_remove_user_operation(&email);
        let req = AlterInboundRequest {
            tag: tag.to_string(),
            operation: Some(op),
        };
        match client.alter_inbound(req).await {
            Ok(_) => {}
            Err(status) if xray::is_not_found(&status) => {}
            Err(status) => warn!(
                grant_id = snapshot.grant_id,
                endpoint_tag = tag,
                %status,
                "quota tick: xray alter_inbound remove_user failed"
            ),
        }
    } else {
        warn!(
            grant_id = snapshot.grant_id,
            "quota tick: missing endpoint tag, skipping xray remove_user"
        );
    }

    if let Err(err) = set_grant_enabled_via_raft(raft, &snapshot.grant_id, false).await {
        warn!(
            grant_id = snapshot.grant_id,
            %err,
            "quota tick: raft disable failed"
        );
        return Ok(());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{collections::BTreeMap, net::SocketAddr};

    use pretty_assertions::assert_eq;
    use tokio::sync::{Mutex, oneshot, watch};

    use crate::{
        domain::{CyclePolicy, CyclePolicyDefault, EndpointKind, Node},
        raft::app::{LocalRaft, RaftFacade},
        state::{DesiredStateCommand, JsonSnapshotStore, StoreInit},
        xray::proto::xray::{
            app::{
                proxyman::command::handler_service_server::{HandlerService, HandlerServiceServer},
                stats::command::stats_service_server::{StatsService, StatsServiceServer},
            },
            common::serial::TypedMessage,
        },
    };

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        RemoveUser { tag: String, email: String },
    }

    #[derive(Debug, Default)]
    struct RecordingState {
        calls: Vec<Call>,
        stats: BTreeMap<String, i64>,
        stats_calls: Vec<String>,
    }

    #[derive(Debug)]
    struct RecordingHandler {
        state: Arc<Mutex<RecordingState>>,
    }

    fn decode_typed<T: prost::Message + Default>(tm: &TypedMessage) -> T {
        T::decode(tm.value.as_slice()).unwrap()
    }

    #[tonic::async_trait]
    impl HandlerService for RecordingHandler {
        async fn add_inbound(
            &self,
            _request: tonic::Request<
                crate::xray::proto::xray::app::proxyman::command::AddInboundRequest,
            >,
        ) -> Result<
            tonic::Response<crate::xray::proto::xray::app::proxyman::command::AddInboundResponse>,
            tonic::Status,
        > {
            Err(tonic::Status::unimplemented("add_inbound"))
        }

        async fn remove_inbound(
            &self,
            _request: tonic::Request<
                crate::xray::proto::xray::app::proxyman::command::RemoveInboundRequest,
            >,
        ) -> Result<
            tonic::Response<
                crate::xray::proto::xray::app::proxyman::command::RemoveInboundResponse,
            >,
            tonic::Status,
        > {
            Err(tonic::Status::unimplemented("remove_inbound"))
        }

        async fn alter_inbound(
            &self,
            request: tonic::Request<
                crate::xray::proto::xray::app::proxyman::command::AlterInboundRequest,
            >,
        ) -> Result<
            tonic::Response<crate::xray::proto::xray::app::proxyman::command::AlterInboundResponse>,
            tonic::Status,
        > {
            let req = request.into_inner();
            let op = req
                .operation
                .ok_or_else(|| tonic::Status::invalid_argument("operation required"))?;
            if op.r#type != "xray.app.proxyman.command.RemoveUserOperation" {
                return Err(tonic::Status::unimplemented(
                    "only RemoveUserOperation supported",
                ));
            }
            let decoded: crate::xray::proto::xray::app::proxyman::command::RemoveUserOperation =
                decode_typed(&op);
            self.state.lock().await.calls.push(Call::RemoveUser {
                tag: req.tag,
                email: decoded.email,
            });
            Ok(tonic::Response::new(
                crate::xray::proto::xray::app::proxyman::command::AlterInboundResponse {},
            ))
        }

        async fn list_inbounds(
            &self,
            _request: tonic::Request<
                crate::xray::proto::xray::app::proxyman::command::ListInboundsRequest,
            >,
        ) -> Result<
            tonic::Response<crate::xray::proto::xray::app::proxyman::command::ListInboundsResponse>,
            tonic::Status,
        > {
            Err(tonic::Status::unimplemented("list_inbounds"))
        }

        async fn get_inbound_users(
            &self,
            _request: tonic::Request<
                crate::xray::proto::xray::app::proxyman::command::GetInboundUserRequest,
            >,
        ) -> Result<
            tonic::Response<
                crate::xray::proto::xray::app::proxyman::command::GetInboundUserResponse,
            >,
            tonic::Status,
        > {
            Err(tonic::Status::unimplemented("get_inbound_users"))
        }

        async fn get_inbound_users_count(
            &self,
            _request: tonic::Request<
                crate::xray::proto::xray::app::proxyman::command::GetInboundUserRequest,
            >,
        ) -> Result<
            tonic::Response<
                crate::xray::proto::xray::app::proxyman::command::GetInboundUsersCountResponse,
            >,
            tonic::Status,
        > {
            Err(tonic::Status::unimplemented("get_inbound_users_count"))
        }

        async fn add_outbound(
            &self,
            _request: tonic::Request<
                crate::xray::proto::xray::app::proxyman::command::AddOutboundRequest,
            >,
        ) -> Result<
            tonic::Response<crate::xray::proto::xray::app::proxyman::command::AddOutboundResponse>,
            tonic::Status,
        > {
            Err(tonic::Status::unimplemented("add_outbound"))
        }

        async fn remove_outbound(
            &self,
            _request: tonic::Request<
                crate::xray::proto::xray::app::proxyman::command::RemoveOutboundRequest,
            >,
        ) -> Result<
            tonic::Response<
                crate::xray::proto::xray::app::proxyman::command::RemoveOutboundResponse,
            >,
            tonic::Status,
        > {
            Err(tonic::Status::unimplemented("remove_outbound"))
        }

        async fn alter_outbound(
            &self,
            _request: tonic::Request<
                crate::xray::proto::xray::app::proxyman::command::AlterOutboundRequest,
            >,
        ) -> Result<
            tonic::Response<
                crate::xray::proto::xray::app::proxyman::command::AlterOutboundResponse,
            >,
            tonic::Status,
        > {
            Err(tonic::Status::unimplemented("alter_outbound"))
        }

        async fn list_outbounds(
            &self,
            _request: tonic::Request<
                crate::xray::proto::xray::app::proxyman::command::ListOutboundsRequest,
            >,
        ) -> Result<
            tonic::Response<
                crate::xray::proto::xray::app::proxyman::command::ListOutboundsResponse,
            >,
            tonic::Status,
        > {
            Err(tonic::Status::unimplemented("list_outbounds"))
        }
    }

    #[derive(Debug)]
    struct RecordingStats {
        state: Arc<Mutex<RecordingState>>,
    }

    #[tonic::async_trait]
    impl StatsService for RecordingStats {
        async fn get_stats(
            &self,
            request: tonic::Request<crate::xray::proto::xray::app::stats::command::GetStatsRequest>,
        ) -> Result<
            tonic::Response<crate::xray::proto::xray::app::stats::command::GetStatsResponse>,
            tonic::Status,
        > {
            let req = request.into_inner();
            let mut state = self.state.lock().await;
            state.stats_calls.push(req.name.clone());
            let value = state
                .stats
                .get(&req.name)
                .copied()
                .ok_or_else(|| tonic::Status::not_found("missing stat"))?;
            Ok(tonic::Response::new(
                crate::xray::proto::xray::app::stats::command::GetStatsResponse {
                    stat: Some(crate::xray::proto::xray::app::stats::command::Stat {
                        name: req.name,
                        value,
                    }),
                },
            ))
        }

        async fn get_stats_online(
            &self,
            _request: tonic::Request<
                crate::xray::proto::xray::app::stats::command::GetStatsRequest,
            >,
        ) -> Result<
            tonic::Response<crate::xray::proto::xray::app::stats::command::GetStatsResponse>,
            tonic::Status,
        > {
            Err(tonic::Status::unimplemented("get_stats_online"))
        }

        async fn query_stats(
            &self,
            _request: tonic::Request<
                crate::xray::proto::xray::app::stats::command::QueryStatsRequest,
            >,
        ) -> Result<
            tonic::Response<crate::xray::proto::xray::app::stats::command::QueryStatsResponse>,
            tonic::Status,
        > {
            Err(tonic::Status::unimplemented("query_stats"))
        }

        async fn get_sys_stats(
            &self,
            _request: tonic::Request<
                crate::xray::proto::xray::app::stats::command::SysStatsRequest,
            >,
        ) -> Result<
            tonic::Response<crate::xray::proto::xray::app::stats::command::SysStatsResponse>,
            tonic::Status,
        > {
            Err(tonic::Status::unimplemented("get_sys_stats"))
        }

        async fn get_stats_online_ip_list(
            &self,
            _request: tonic::Request<
                crate::xray::proto::xray::app::stats::command::GetStatsRequest,
            >,
        ) -> Result<
            tonic::Response<
                crate::xray::proto::xray::app::stats::command::GetStatsOnlineIpListResponse,
            >,
            tonic::Status,
        > {
            Err(tonic::Status::unimplemented("get_stats_online_ip_list"))
        }
    }

    async fn start_server(state: Arc<Mutex<RecordingState>>) -> (SocketAddr, oneshot::Sender<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);

        let handler = RecordingHandler {
            state: state.clone(),
        };
        let stats = RecordingStats { state };
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(HandlerServiceServer::new(handler))
                .add_service(StatsServiceServer::new(stats))
                .serve_with_incoming_shutdown(incoming, async move {
                    let _ = shutdown_rx.await;
                })
                .await;
        });

        (addr, shutdown_tx)
    }

    fn test_store_init(
        tmp_dir: &std::path::Path,
        xray_api_addr: SocketAddr,
        quota_auto_unban: bool,
    ) -> (Config, Arc<Mutex<JsonSnapshotStore>>) {
        let config = Config {
            bind: SocketAddr::from(([127, 0, 0, 1], 0)),
            xray_api_addr,
            data_dir: tmp_dir.to_path_buf(),
            admin_token: "testtoken".to_string(),
            node_name: "node-1".to_string(),
            public_domain: "".to_string(),
            api_base_url: "https://127.0.0.1:62416".to_string(),
            quota_poll_interval_secs: 10,
            quota_auto_unban,
        };

        let store = JsonSnapshotStore::load_or_init(StoreInit {
            data_dir: config.data_dir.clone(),
            bootstrap_node_id: None,
            bootstrap_node_name: config.node_name.clone(),
            bootstrap_public_domain: config.public_domain.clone(),
            bootstrap_api_base_url: config.api_base_url.clone(),
        })
        .unwrap();

        (config, Arc::new(Mutex::new(store)))
    }

    fn stat_name(email: &str, direction: &str) -> String {
        format!("user>>>{email}>>>traffic>>>{direction}")
    }

    fn test_raft(store: Arc<Mutex<JsonSnapshotStore>>) -> Arc<dyn RaftFacade> {
        let (_tx, metrics) = watch::channel(openraft::RaftMetrics::<
            crate::raft::types::NodeId,
            crate::raft::types::NodeMeta,
        >::new_initial(0));
        Arc::new(LocalRaft::new(store, metrics))
    }

    #[derive(Clone)]
    struct RecordingRaft {
        inner: Arc<dyn RaftFacade>,
        calls: Arc<Mutex<Vec<DesiredStateCommand>>>,
    }

    impl RaftFacade for RecordingRaft {
        fn metrics(
            &self,
        ) -> watch::Receiver<
            openraft::RaftMetrics<crate::raft::types::NodeId, crate::raft::types::NodeMeta>,
        > {
            self.inner.metrics()
        }

        fn client_write(
            &self,
            cmd: DesiredStateCommand,
        ) -> crate::raft::app::BoxFuture<'_, anyhow::Result<crate::raft::types::ClientResponse>>
        {
            let inner = self.inner.clone();
            let calls = self.calls.clone();
            Box::pin(async move {
                calls.lock().await.push(cmd.clone());
                inner.client_write(cmd).await
            })
        }

        fn add_learner(
            &self,
            node_id: crate::raft::types::NodeId,
            node: crate::raft::types::NodeMeta,
        ) -> crate::raft::app::BoxFuture<'_, anyhow::Result<()>> {
            self.inner.add_learner(node_id, node)
        }

        fn add_voters(
            &self,
            node_ids: std::collections::BTreeSet<crate::raft::types::NodeId>,
        ) -> crate::raft::app::BoxFuture<'_, anyhow::Result<()>> {
            self.inner.add_voters(node_ids)
        }
    }

    fn recording_raft(
        store: Arc<Mutex<JsonSnapshotStore>>,
    ) -> (Arc<dyn RaftFacade>, Arc<Mutex<Vec<DesiredStateCommand>>>) {
        let inner = test_raft(store);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let raft = RecordingRaft {
            inner,
            calls: calls.clone(),
        };
        (Arc::new(raft), calls)
    }

    #[derive(Clone)]
    struct FailOnceRaft {
        inner: Arc<dyn RaftFacade>,
        failed: Arc<Mutex<bool>>,
    }

    impl RaftFacade for FailOnceRaft {
        fn metrics(
            &self,
        ) -> watch::Receiver<
            openraft::RaftMetrics<crate::raft::types::NodeId, crate::raft::types::NodeMeta>,
        > {
            self.inner.metrics()
        }

        fn client_write(
            &self,
            cmd: DesiredStateCommand,
        ) -> crate::raft::app::BoxFuture<'_, anyhow::Result<crate::raft::types::ClientResponse>>
        {
            let inner = self.inner.clone();
            let failed = self.failed.clone();
            Box::pin(async move {
                if matches!(
                    cmd,
                    DesiredStateCommand::SetGrantEnabled {
                        enabled: false,
                        source: GrantEnabledSource::Quota,
                        ..
                    }
                ) {
                    let mut failed = failed.lock().await;
                    if !*failed {
                        *failed = true;
                        return Err(anyhow::anyhow!("injected raft failure"));
                    }
                }
                inner.client_write(cmd).await
            })
        }

        fn add_learner(
            &self,
            node_id: crate::raft::types::NodeId,
            node: crate::raft::types::NodeMeta,
        ) -> crate::raft::app::BoxFuture<'_, anyhow::Result<()>> {
            self.inner.add_learner(node_id, node)
        }

        fn add_voters(
            &self,
            node_ids: std::collections::BTreeSet<crate::raft::types::NodeId>,
        ) -> crate::raft::app::BoxFuture<'_, anyhow::Result<()>> {
            self.inner.add_voters(node_ids)
        }
    }

    fn fail_once_raft(store: Arc<Mutex<JsonSnapshotStore>>) -> Arc<dyn RaftFacade> {
        Arc::new(FailOnceRaft {
            inner: test_raft(store),
            failed: Arc::new(Mutex::new(false)),
        })
    }

    #[tokio::test]
    async fn poll_updates_usage() {
        let state = Arc::new(Mutex::new(RecordingState::default()));
        let (addr, shutdown) = start_server(state.clone()).await;

        let tmp = tempfile::tempdir().unwrap();
        let (config, store) = test_store_init(tmp.path(), addr, true);
        let raft = test_raft(store.clone());

        let grant_id = {
            let mut store = store.lock().await;
            let local_node_id = store.list_nodes()[0].node_id.clone();
            let user = store
                .create_user("alice".to_string(), CyclePolicyDefault::ByUser, 1)
                .unwrap();
            let endpoint = store
                .create_endpoint(
                    local_node_id,
                    EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                    8388,
                    serde_json::json!({}),
                )
                .unwrap();
            let grant = store
                .create_grant(
                    user.user_id,
                    endpoint.endpoint_id,
                    0,
                    CyclePolicy::InheritUser,
                    None,
                    None,
                )
                .unwrap();
            grant.grant_id
        };

        let email = format!("grant:{grant_id}");
        {
            let mut st = state.lock().await;
            st.stats.insert(stat_name(&email, "uplink"), 100);
            st.stats.insert(stat_name(&email, "downlink"), 200);
        }

        let reconcile = ReconcileHandle::noop();
        let now = DateTime::parse_from_rfc3339("2025-12-18T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        run_quota_tick_at(now, &config, &store, &reconcile, &raft)
            .await
            .unwrap();

        {
            let mut st = state.lock().await;
            st.stats.insert(stat_name(&email, "uplink"), 150);
            st.stats.insert(stat_name(&email, "downlink"), 250);
        }
        run_quota_tick_at(now, &config, &store, &reconcile, &raft)
            .await
            .unwrap();

        let store = store.lock().await;
        let usage = store.get_grant_usage(&grant_id).unwrap();
        assert_eq!(usage.used_bytes, 400);

        let _ = shutdown.send(());
    }

    #[tokio::test]
    async fn remote_grant_does_not_call_xray_or_create_usage() {
        let state = Arc::new(Mutex::new(RecordingState::default()));
        let (addr, shutdown) = start_server(state.clone()).await;

        let tmp = tempfile::tempdir().unwrap();
        let (config, store) = test_store_init(tmp.path(), addr, true);
        let raft = test_raft(store.clone());

        let grant_id = {
            let mut store = store.lock().await;
            let remote_node_id = "node-remote".to_string();
            let _ = store
                .upsert_node(Node {
                    node_id: remote_node_id.clone(),
                    node_name: "node-2".to_string(),
                    public_domain: "".to_string(),
                    api_base_url: "https://127.0.0.1:62417".to_string(),
                })
                .unwrap();

            let user = store
                .create_user("alice".to_string(), CyclePolicyDefault::ByUser, 1)
                .unwrap();
            let endpoint = store
                .create_endpoint(
                    remote_node_id,
                    EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                    8388,
                    serde_json::json!({}),
                )
                .unwrap();
            let grant = store
                .create_grant(
                    user.user_id,
                    endpoint.endpoint_id,
                    0,
                    CyclePolicy::InheritUser,
                    None,
                    None,
                )
                .unwrap();
            grant.grant_id
        };

        let email = format!("grant:{grant_id}");
        {
            let mut st = state.lock().await;
            st.stats.insert(stat_name(&email, "uplink"), 100);
            st.stats.insert(stat_name(&email, "downlink"), 200);
        }

        let reconcile = ReconcileHandle::noop();
        let now = DateTime::parse_from_rfc3339("2025-12-18T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        run_quota_tick_at(now, &config, &store, &reconcile, &raft)
            .await
            .unwrap();

        let st = state.lock().await;
        assert_eq!(st.calls, vec![]);
        assert!(st.stats_calls.is_empty());
        drop(st);

        let store_guard = store.lock().await;
        assert_eq!(store_guard.get_grant_usage(&grant_id), None);

        let _ = shutdown.send(());
    }

    #[tokio::test]
    async fn remote_grant_is_ignored_when_local_grant_exists() {
        let state = Arc::new(Mutex::new(RecordingState::default()));
        let (addr, shutdown) = start_server(state.clone()).await;

        let tmp = tempfile::tempdir().unwrap();
        let (config, store) = test_store_init(tmp.path(), addr, true);
        let raft = test_raft(store.clone());

        let (local_grant_id, remote_grant_id) = {
            let mut store = store.lock().await;
            let local_node_id = store.list_nodes()[0].node_id.clone();
            let remote_node_id = "node-remote".to_string();
            let _ = store
                .upsert_node(Node {
                    node_id: remote_node_id.clone(),
                    node_name: "node-2".to_string(),
                    public_domain: "".to_string(),
                    api_base_url: "https://127.0.0.1:62417".to_string(),
                })
                .unwrap();

            let user = store
                .create_user("alice".to_string(), CyclePolicyDefault::ByUser, 1)
                .unwrap();

            let local_endpoint = store
                .create_endpoint(
                    local_node_id,
                    EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                    8388,
                    serde_json::json!({}),
                )
                .unwrap();
            let remote_endpoint = store
                .create_endpoint(
                    remote_node_id,
                    EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                    8389,
                    serde_json::json!({}),
                )
                .unwrap();

            let local_grant = store
                .create_grant(
                    user.user_id.clone(),
                    local_endpoint.endpoint_id,
                    0,
                    CyclePolicy::InheritUser,
                    None,
                    None,
                )
                .unwrap();
            let remote_grant = store
                .create_grant(
                    user.user_id,
                    remote_endpoint.endpoint_id,
                    0,
                    CyclePolicy::InheritUser,
                    None,
                    None,
                )
                .unwrap();
            (local_grant.grant_id, remote_grant.grant_id)
        };

        let local_email = format!("grant:{local_grant_id}");
        let remote_email = format!("grant:{remote_grant_id}");
        {
            let mut st = state.lock().await;
            st.stats.insert(stat_name(&local_email, "uplink"), 100);
            st.stats.insert(stat_name(&local_email, "downlink"), 200);
            st.stats.insert(stat_name(&remote_email, "uplink"), 300);
            st.stats.insert(stat_name(&remote_email, "downlink"), 400);
        }

        let reconcile = ReconcileHandle::noop();
        let now = DateTime::parse_from_rfc3339("2025-12-18T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        run_quota_tick_at(now, &config, &store, &reconcile, &raft)
            .await
            .unwrap();

        let st = state.lock().await;
        assert!(!st.stats_calls.is_empty());
        assert!(
            !st.stats_calls
                .iter()
                .any(|name| name == &stat_name(&remote_email, "uplink"))
        );
        assert!(
            !st.stats_calls
                .iter()
                .any(|name| name == &stat_name(&remote_email, "downlink"))
        );
        drop(st);

        let store_guard = store.lock().await;
        assert!(store_guard.get_grant_usage(&local_grant_id).is_some());
        assert_eq!(store_guard.get_grant_usage(&remote_grant_id), None);

        let _ = shutdown.send(());
    }

    #[tokio::test]
    async fn exceed_triggers_ban() {
        let state = Arc::new(Mutex::new(RecordingState::default()));
        let (addr, shutdown) = start_server(state.clone()).await;

        let tmp = tempfile::tempdir().unwrap();
        let (config, store) = test_store_init(tmp.path(), addr, true);
        let (raft, raft_calls) = recording_raft(store.clone());

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let reconcile = ReconcileHandle::from_sender(tx);

        let (grant_id, endpoint_tag) = {
            let mut store = store.lock().await;
            let local_node_id = store.list_nodes()[0].node_id.clone();
            let user = store
                .create_user("alice".to_string(), CyclePolicyDefault::ByUser, 1)
                .unwrap();
            let endpoint = store
                .create_endpoint(
                    local_node_id,
                    EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                    8388,
                    serde_json::json!({}),
                )
                .unwrap();
            let grant = store
                .create_grant(
                    user.user_id,
                    endpoint.endpoint_id.clone(),
                    QUOTA_TOLERANCE_BYTES + 100,
                    CyclePolicy::InheritUser,
                    None,
                    None,
                )
                .unwrap();
            (grant.grant_id, endpoint.tag)
        };

        let email = format!("grant:{grant_id}");
        {
            let mut st = state.lock().await;
            st.stats.insert(stat_name(&email, "uplink"), 100);
            st.stats.insert(stat_name(&email, "downlink"), 0);
        }

        let now = DateTime::parse_from_rfc3339("2025-12-18T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        run_quota_tick_at(now, &config, &store, &reconcile, &raft)
            .await
            .unwrap();

        let store_guard = store.lock().await;
        let grant = store_guard.get_grant(&grant_id).unwrap();
        assert!(!grant.enabled);
        let usage = store_guard.get_grant_usage(&grant_id).unwrap();
        assert!(usage.quota_banned);
        assert!(usage.quota_banned_at.is_some());
        drop(store_guard);

        let st = state.lock().await;
        assert!(st.calls.iter().any(|c| matches!(c, Call::RemoveUser { tag, email: e } if tag == &endpoint_tag && e == &email)));

        assert!(
            rx.try_recv().is_ok(),
            "expected quota enforcement to request reconcile"
        );

        let calls = raft_calls.lock().await.clone();
        assert!(calls.iter().any(|cmd| {
            matches!(
                cmd,
                DesiredStateCommand::SetGrantEnabled {
                    grant_id: cmd_grant_id,
                    enabled: false,
                    source: GrantEnabledSource::Quota,
                } if cmd_grant_id == &grant_id
            )
        }));

        let _ = shutdown.send(());
    }

    #[tokio::test]
    async fn quota_raft_retry_disables_grant_after_first_failure() {
        let state = Arc::new(Mutex::new(RecordingState::default()));
        let (addr, shutdown) = start_server(state.clone()).await;

        let tmp = tempfile::tempdir().unwrap();
        let (config, store) = test_store_init(tmp.path(), addr, true);
        let raft = fail_once_raft(store.clone());

        let now = DateTime::parse_from_rfc3339("2025-12-18T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let grant_id = {
            let mut store = store.lock().await;
            let local_node_id = store.list_nodes()[0].node_id.clone();
            let user = store
                .create_user("alice".to_string(), CyclePolicyDefault::ByUser, 1)
                .unwrap();
            let endpoint = store
                .create_endpoint(
                    local_node_id,
                    EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                    8388,
                    serde_json::json!({}),
                )
                .unwrap();
            let grant = store
                .create_grant(
                    user.user_id,
                    endpoint.endpoint_id,
                    0,
                    CyclePolicy::InheritUser,
                    None,
                    None,
                )
                .unwrap();
            let (start, end) =
                current_cycle_window_at(crate::cycle::EffectiveCyclePolicy::ByUser, 1, now)
                    .unwrap();
            store
                .apply_grant_usage_sample(
                    &grant.grant_id,
                    start.to_rfc3339(),
                    end.to_rfc3339(),
                    0,
                    0,
                    now.to_rfc3339(),
                )
                .unwrap();
            store
                .set_quota_banned(&grant.grant_id, "2025-12-18T00:00:00Z".to_string())
                .unwrap();
            grant.grant_id
        };

        let email = format!("grant:{grant_id}");
        {
            let mut st = state.lock().await;
            st.stats.insert(stat_name(&email, "uplink"), 0);
            st.stats.insert(stat_name(&email, "downlink"), 0);
        }

        let reconcile = ReconcileHandle::noop();
        run_quota_tick_at(now, &config, &store, &reconcile, &raft)
            .await
            .unwrap();

        let store_guard = store.lock().await;
        let grant = store_guard.get_grant(&grant_id).unwrap();
        assert!(grant.enabled);
        let usage = store_guard.get_grant_usage(&grant_id).unwrap();
        assert!(usage.quota_banned);
        drop(store_guard);

        run_quota_tick_at(now, &config, &store, &reconcile, &raft)
            .await
            .unwrap();

        let store_guard = store.lock().await;
        let grant = store_guard.get_grant(&grant_id).unwrap();
        assert!(!grant.enabled);

        let _ = shutdown.send(());
    }

    #[tokio::test]
    async fn cycle_rollover_auto_unban() {
        let state = Arc::new(Mutex::new(RecordingState::default()));
        let (addr, shutdown) = start_server(state.clone()).await;

        let tmp = tempfile::tempdir().unwrap();
        let (config, store) = test_store_init(tmp.path(), addr, true);
        let raft = test_raft(store.clone());

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let reconcile = ReconcileHandle::from_sender(tx);

        let grant_id = {
            let mut store = store.lock().await;
            let local_node_id = store.list_nodes()[0].node_id.clone();
            let user = store
                .create_user("alice".to_string(), CyclePolicyDefault::ByUser, 1)
                .unwrap();
            let endpoint = store
                .create_endpoint(
                    local_node_id,
                    EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                    8388,
                    serde_json::json!({}),
                )
                .unwrap();
            let grant = store
                .create_grant(
                    user.user_id,
                    endpoint.endpoint_id,
                    1,
                    CyclePolicy::InheritUser,
                    None,
                    None,
                )
                .unwrap();
            store
                .set_grant_enabled(&grant.grant_id, false, GrantEnabledSource::Quota)
                .unwrap();

            let old_now = DateTime::parse_from_rfc3339("2025-11-15T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc);
            let (start, end) =
                current_cycle_window_at(crate::cycle::EffectiveCyclePolicy::ByUser, 1, old_now)
                    .unwrap();
            store
                .apply_grant_usage_sample(
                    &grant.grant_id,
                    start.to_rfc3339(),
                    end.to_rfc3339(),
                    0,
                    0,
                    old_now.to_rfc3339(),
                )
                .unwrap();
            store
                .set_quota_banned(&grant.grant_id, "2025-11-15T00:00:00Z".to_string())
                .unwrap();
            grant.grant_id
        };

        let email = format!("grant:{grant_id}");
        {
            let mut st = state.lock().await;
            st.stats.insert(stat_name(&email, "uplink"), 0);
            st.stats.insert(stat_name(&email, "downlink"), 0);
        }

        let new_now = DateTime::parse_from_rfc3339("2025-12-02T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        run_quota_tick_at(new_now, &config, &store, &reconcile, &raft)
            .await
            .unwrap();

        let store_guard = store.lock().await;
        let grant = store_guard.get_grant(&grant_id).unwrap();
        assert!(grant.enabled);
        let usage = store_guard.get_grant_usage(&grant_id).unwrap();
        assert!(!usage.quota_banned);
        assert_eq!(usage.quota_banned_at, None);
        assert_eq!(usage.used_bytes, 0);

        assert!(
            rx.try_recv().is_ok(),
            "expected auto-unban to request reconcile"
        );

        let _ = shutdown.send(());
    }

    #[tokio::test]
    async fn manual_disabled_not_auto_unbanned() {
        let state = Arc::new(Mutex::new(RecordingState::default()));
        let (addr, shutdown) = start_server(state.clone()).await;

        let tmp = tempfile::tempdir().unwrap();
        let (config, store) = test_store_init(tmp.path(), addr, true);
        let raft = test_raft(store.clone());

        let grant_id = {
            let mut store = store.lock().await;
            let local_node_id = store.list_nodes()[0].node_id.clone();
            let user = store
                .create_user("alice".to_string(), CyclePolicyDefault::ByUser, 1)
                .unwrap();
            let endpoint = store
                .create_endpoint(
                    local_node_id,
                    EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                    8388,
                    serde_json::json!({}),
                )
                .unwrap();
            let grant = store
                .create_grant(
                    user.user_id,
                    endpoint.endpoint_id,
                    1,
                    CyclePolicy::InheritUser,
                    None,
                    None,
                )
                .unwrap();
            store
                .set_grant_enabled(&grant.grant_id, false, GrantEnabledSource::Manual)
                .unwrap();

            let old_now = DateTime::parse_from_rfc3339("2025-11-15T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc);
            let (start, end) =
                current_cycle_window_at(crate::cycle::EffectiveCyclePolicy::ByUser, 1, old_now)
                    .unwrap();
            store
                .apply_grant_usage_sample(
                    &grant.grant_id,
                    start.to_rfc3339(),
                    end.to_rfc3339(),
                    0,
                    0,
                    old_now.to_rfc3339(),
                )
                .unwrap();
            store.clear_quota_banned(&grant.grant_id).unwrap();
            grant.grant_id
        };

        let email = format!("grant:{grant_id}");
        {
            let mut st = state.lock().await;
            st.stats.insert(stat_name(&email, "uplink"), 0);
            st.stats.insert(stat_name(&email, "downlink"), 0);
        }

        let reconcile = ReconcileHandle::noop();
        let new_now = DateTime::parse_from_rfc3339("2025-12-02T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        run_quota_tick_at(new_now, &config, &store, &reconcile, &raft)
            .await
            .unwrap();

        let store_guard = store.lock().await;
        let grant = store_guard.get_grant(&grant_id).unwrap();
        assert!(!grant.enabled);
        let usage = store_guard.get_grant_usage(&grant_id).unwrap();
        assert!(!usage.quota_banned);

        let _ = shutdown.send(());
    }

    #[tokio::test]
    async fn missing_endpoint_tag_still_disables() {
        let state = Arc::new(Mutex::new(RecordingState::default()));
        let (addr, shutdown) = start_server(state.clone()).await;

        let tmp = tempfile::tempdir().unwrap();
        let (config, store) = test_store_init(tmp.path(), addr, true);
        let raft = test_raft(store.clone());

        let grant_id = {
            let mut store = store.lock().await;
            let local_node_id = store.list_nodes()[0].node_id.clone();
            let user = store
                .create_user("alice".to_string(), CyclePolicyDefault::ByUser, 1)
                .unwrap();
            let endpoint = store
                .create_endpoint(
                    local_node_id,
                    EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                    8388,
                    serde_json::json!({}),
                )
                .unwrap();
            let grant = store
                .create_grant(
                    user.user_id,
                    endpoint.endpoint_id.clone(),
                    QUOTA_TOLERANCE_BYTES + 100,
                    CyclePolicy::InheritUser,
                    None,
                    None,
                )
                .unwrap();
            assert!(store.delete_endpoint(&endpoint.endpoint_id).unwrap());
            grant.grant_id
        };

        let email = format!("grant:{grant_id}");
        {
            let mut st = state.lock().await;
            st.stats.insert(stat_name(&email, "uplink"), 100);
            st.stats.insert(stat_name(&email, "downlink"), 0);
        }

        let reconcile = ReconcileHandle::noop();
        let now = DateTime::parse_from_rfc3339("2025-12-18T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        run_quota_tick_at(now, &config, &store, &reconcile, &raft)
            .await
            .unwrap();

        let store_guard = store.lock().await;
        let grant = store_guard.get_grant(&grant_id).unwrap();
        assert!(!grant.enabled);
        let usage = store_guard.get_grant_usage(&grant_id).unwrap();
        assert!(usage.quota_banned);
        assert!(usage.quota_banned_at.is_some());
        drop(store_guard);

        let st = state.lock().await;
        assert_eq!(st.calls, vec![]);

        let _ = shutdown.send(());
    }

    #[tokio::test]
    async fn rollover_does_not_auto_unban_when_disabled_in_config() {
        let state = Arc::new(Mutex::new(RecordingState::default()));
        let (addr, shutdown) = start_server(state.clone()).await;

        let tmp = tempfile::tempdir().unwrap();
        let (config, store) = test_store_init(tmp.path(), addr, false);
        let raft = test_raft(store.clone());

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let reconcile = ReconcileHandle::from_sender(tx);

        let banned_at = "2025-11-15T00:00:00Z".to_string();
        let grant_id = {
            let mut store = store.lock().await;
            let local_node_id = store.list_nodes()[0].node_id.clone();
            let user = store
                .create_user("alice".to_string(), CyclePolicyDefault::ByUser, 1)
                .unwrap();
            let endpoint = store
                .create_endpoint(
                    local_node_id,
                    EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                    8388,
                    serde_json::json!({}),
                )
                .unwrap();
            let grant = store
                .create_grant(
                    user.user_id,
                    endpoint.endpoint_id,
                    1,
                    CyclePolicy::InheritUser,
                    None,
                    None,
                )
                .unwrap();
            store
                .set_grant_enabled(&grant.grant_id, false, GrantEnabledSource::Quota)
                .unwrap();

            let old_now = DateTime::parse_from_rfc3339("2025-11-15T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc);
            let (start, end) =
                current_cycle_window_at(crate::cycle::EffectiveCyclePolicy::ByUser, 1, old_now)
                    .unwrap();
            store
                .apply_grant_usage_sample(
                    &grant.grant_id,
                    start.to_rfc3339(),
                    end.to_rfc3339(),
                    0,
                    0,
                    old_now.to_rfc3339(),
                )
                .unwrap();
            store
                .set_quota_banned(&grant.grant_id, banned_at.clone())
                .unwrap();
            grant.grant_id
        };

        let email = format!("grant:{grant_id}");
        {
            let mut st = state.lock().await;
            st.stats.insert(stat_name(&email, "uplink"), 0);
            st.stats.insert(stat_name(&email, "downlink"), 0);
        }

        let new_now = DateTime::parse_from_rfc3339("2025-12-02T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        run_quota_tick_at(new_now, &config, &store, &reconcile, &raft)
            .await
            .unwrap();

        let store_guard = store.lock().await;
        let grant = store_guard.get_grant(&grant_id).unwrap();
        assert!(!grant.enabled);
        let usage = store_guard.get_grant_usage(&grant_id).unwrap();
        assert!(usage.quota_banned);
        assert_eq!(usage.quota_banned_at, Some(banned_at));

        assert!(
            rx.try_recv().is_err(),
            "expected quota_auto_unban=false to not request reconcile"
        );

        let _ = shutdown.send(());
    }

    #[tokio::test]
    async fn xray_connect_failure_is_non_fatal_and_does_not_create_usage() {
        let addr = SocketAddr::from(([127, 0, 0, 1], 0));

        let tmp = tempfile::tempdir().unwrap();
        let (config, store) = test_store_init(tmp.path(), addr, true);
        let raft = test_raft(store.clone());

        let grant_id = {
            let mut store = store.lock().await;
            let local_node_id = store.list_nodes()[0].node_id.clone();
            let user = store
                .create_user("alice".to_string(), CyclePolicyDefault::ByUser, 1)
                .unwrap();
            let endpoint = store
                .create_endpoint(
                    local_node_id,
                    EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                    8388,
                    serde_json::json!({}),
                )
                .unwrap();
            let grant = store
                .create_grant(
                    user.user_id,
                    endpoint.endpoint_id,
                    1,
                    CyclePolicy::InheritUser,
                    None,
                    None,
                )
                .unwrap();
            grant.grant_id
        };

        let reconcile = ReconcileHandle::noop();
        let now = DateTime::parse_from_rfc3339("2025-12-18T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(
            run_quota_tick_at(now, &config, &store, &reconcile, &raft)
                .await
                .is_ok()
        );

        let store_guard = store.lock().await;
        assert_eq!(store_guard.get_grant_usage(&grant_id), None);
    }

    #[tokio::test]
    async fn invalid_stats_values_do_not_corrupt_usage() {
        let state = Arc::new(Mutex::new(RecordingState::default()));
        let (addr, shutdown) = start_server(state.clone()).await;

        let tmp = tempfile::tempdir().unwrap();
        let (config, store) = test_store_init(tmp.path(), addr, true);
        let raft = test_raft(store.clone());

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let reconcile = ReconcileHandle::from_sender(tx);

        let now = DateTime::parse_from_rfc3339("2025-12-18T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let grant_id = {
            let mut store = store.lock().await;
            let local_node_id = store.list_nodes()[0].node_id.clone();
            let user = store
                .create_user("alice".to_string(), CyclePolicyDefault::ByUser, 1)
                .unwrap();
            let endpoint = store
                .create_endpoint(
                    local_node_id,
                    EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                    8388,
                    serde_json::json!({}),
                )
                .unwrap();
            let grant = store
                .create_grant(
                    user.user_id,
                    endpoint.endpoint_id,
                    u64::MAX,
                    CyclePolicy::InheritUser,
                    None,
                    None,
                )
                .unwrap();
            let (start, end) =
                current_cycle_window_at(crate::cycle::EffectiveCyclePolicy::ByUser, 1, now)
                    .unwrap();
            store
                .apply_grant_usage_sample(
                    &grant.grant_id,
                    start.to_rfc3339(),
                    end.to_rfc3339(),
                    100,
                    200,
                    now.to_rfc3339(),
                )
                .unwrap();
            grant.grant_id
        };

        let email = format!("grant:{grant_id}");
        {
            let mut st = state.lock().await;
            st.stats.insert(stat_name(&email, "uplink"), -1);
        }

        run_quota_tick_at(now, &config, &store, &reconcile, &raft)
            .await
            .unwrap();

        let store_guard = store.lock().await;
        let grant = store_guard.get_grant(&grant_id).unwrap();
        assert!(grant.enabled);
        let usage = store_guard.get_grant_usage(&grant_id).unwrap();
        assert_eq!(usage.used_bytes, 300);
        assert!(!usage.quota_banned);
        drop(store_guard);

        let st = state.lock().await;
        assert_eq!(st.calls, vec![]);
        drop(st);

        assert!(
            rx.try_recv().is_err(),
            "expected invalid stats to not trigger reconcile"
        );

        let _ = shutdown.send(());
    }
}
