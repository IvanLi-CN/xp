use std::{net::SocketAddr, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use tokio::{
    sync::RwLock,
    time::{Instant, MissedTickBehavior},
};
use tracing::{debug, info, warn};

use crate::{
    config::Config,
    reconcile::ReconcileHandle,
    xray::proto::xray::app::stats::command::{
        GetStatsRequest, stats_service_client::StatsServiceClient,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XrayStatus {
    Unknown,
    Up,
    Down,
}

impl XrayStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Up => "up",
            Self::Down => "down",
        }
    }
}

#[derive(Debug, Clone)]
pub struct XrayHealthSnapshot {
    pub status: XrayStatus,
    pub last_ok_at: Option<DateTime<Utc>>,
    pub last_fail_at: Option<DateTime<Utc>>,
    pub down_since: Option<DateTime<Utc>>,
    pub consecutive_failures: u32,
    pub recoveries_observed: u64,
}

impl Default for XrayHealthSnapshot {
    fn default() -> Self {
        Self {
            status: XrayStatus::Unknown,
            last_ok_at: None,
            last_fail_at: None,
            down_since: None,
            consecutive_failures: 0,
            recoveries_observed: 0,
        }
    }
}

#[derive(Clone)]
pub struct XrayHealthHandle {
    inner: Arc<RwLock<XrayHealthSnapshot>>,
}

impl XrayHealthHandle {
    pub fn new_unknown() -> Self {
        Self {
            inner: Arc::new(RwLock::new(XrayHealthSnapshot::default())),
        }
    }

    pub async fn snapshot(&self) -> XrayHealthSnapshot {
        self.inner.read().await.clone()
    }
}

#[derive(Debug, Clone)]
pub struct XraySupervisorOptions {
    pub interval: Duration,
    pub fails_before_down: u32,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub down_log_throttle: Duration,
}

impl XraySupervisorOptions {
    pub fn from_config(config: &Config) -> Self {
        Self {
            interval: Duration::from_secs(config.xray_health_interval_secs),
            fails_before_down: config.xray_health_fails_before_down as u32,
            connect_timeout: Duration::from_millis(500),
            request_timeout: Duration::from_millis(500),
            down_log_throttle: Duration::from_secs(30),
        }
    }
}

pub fn spawn_xray_supervisor(
    config: Arc<Config>,
    reconcile: ReconcileHandle,
) -> (XrayHealthHandle, tokio::task::JoinHandle<()>) {
    let opts = XraySupervisorOptions::from_config(&config);
    spawn_xray_supervisor_with_options(config.xray_api_addr, opts, reconcile)
}

pub fn spawn_xray_supervisor_with_options(
    xray_api_addr: SocketAddr,
    opts: XraySupervisorOptions,
    reconcile: ReconcileHandle,
) -> (XrayHealthHandle, tokio::task::JoinHandle<()>) {
    let health = XrayHealthHandle::new_unknown();
    let health_clone = health.clone();

    let task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(opts.interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        // Avoid log spam while Down by throttling periodic warnings.
        let mut last_down_warn_at: Option<Instant> = None;

        loop {
            interval.tick().await;

            let now = Utc::now();
            let probe =
                probe_xray_grpc(xray_api_addr, opts.connect_timeout, opts.request_timeout).await;

            let mut snap = health_clone.inner.write().await;
            let prev = snap.status;

            match probe {
                Ok(()) => {
                    snap.last_ok_at = Some(now);
                    snap.consecutive_failures = 0;

                    if prev == XrayStatus::Down {
                        snap.status = XrayStatus::Up;
                        snap.down_since = None;
                        snap.recoveries_observed = snap.recoveries_observed.saturating_add(1);

                        info!(
                            xray_status = snap.status.as_str(),
                            recoveries_observed = snap.recoveries_observed,
                            "xray recovered (down -> up); requesting full reconcile"
                        );
                        reconcile.request_full();
                    } else if prev != XrayStatus::Up {
                        snap.status = XrayStatus::Up;
                        info!(xray_status = snap.status.as_str(), "xray became available");
                    } else {
                        debug!(xray_status = snap.status.as_str(), "xray probe ok");
                    }

                    last_down_warn_at = None;
                }
                Err(err) => {
                    snap.last_fail_at = Some(now);
                    snap.consecutive_failures = snap.consecutive_failures.saturating_add(1);

                    let should_mark_down = snap.consecutive_failures >= opts.fails_before_down
                        && prev != XrayStatus::Down;

                    if should_mark_down {
                        snap.status = XrayStatus::Down;
                        snap.down_since = Some(now);
                        warn!(
                            xray_status = snap.status.as_str(),
                            consecutive_failures = snap.consecutive_failures,
                            error = %err,
                            "xray marked down"
                        );
                        last_down_warn_at = Some(Instant::now());
                        continue;
                    }

                    // Throttle warnings while in Down to avoid log spam.
                    if snap.status == XrayStatus::Down {
                        let now_i = Instant::now();
                        let should_warn = last_down_warn_at
                            .map(|t| now_i.duration_since(t) >= opts.down_log_throttle)
                            .unwrap_or(true);
                        if should_warn {
                            warn!(
                                xray_status = snap.status.as_str(),
                                consecutive_failures = snap.consecutive_failures,
                                error = %err,
                                "xray still down"
                            );
                            last_down_warn_at = Some(now_i);
                        } else {
                            debug!(
                                xray_status = snap.status.as_str(),
                                consecutive_failures = snap.consecutive_failures,
                                error = %err,
                                "xray probe failed (throttled)"
                            );
                        }
                    } else if prev != XrayStatus::Unknown {
                        // Unknown is expected during startup; don't warn until we were up at least once.
                        debug!(
                            xray_status = snap.status.as_str(),
                            consecutive_failures = snap.consecutive_failures,
                            error = %err,
                            "xray probe failed"
                        );
                    } else {
                        debug!(
                            xray_status = snap.status.as_str(),
                            consecutive_failures = snap.consecutive_failures,
                            error = %err,
                            "xray probe failed (startup)"
                        );
                    }
                }
            }
        }
    });

    (health, task)
}

async fn probe_xray_grpc(
    xray_api_addr: SocketAddr,
    connect_timeout: Duration,
    request_timeout: Duration,
) -> Result<(), ProbeError> {
    // 1) Establish an HTTP/2 connection to the local gRPC endpoint.
    let endpoint = tonic::transport::Endpoint::from_shared(format!("http://{xray_api_addr}"))?
        .connect_timeout(connect_timeout)
        .timeout(request_timeout);
    let channel = endpoint.connect().await.map_err(ProbeError::Transport)?;

    // 2) Do a cheap call. `GetStats` is universally present; a NotFound response still proves gRPC is alive.
    let mut client = StatsServiceClient::new(channel);
    let req = GetStatsRequest {
        name: "xp.healthcheck".to_string(),
        reset: false,
    };

    match client.get_stats(req).await {
        Ok(_) => Ok(()),
        Err(status) if status.code() == tonic::Code::NotFound => Ok(()),
        Err(status)
            if matches!(
                status.code(),
                tonic::Code::Unavailable | tonic::Code::DeadlineExceeded | tonic::Code::Cancelled
            ) =>
        {
            Err(ProbeError::GrpcUnavailable(status))
        }
        Err(_status) => Ok(()),
    }
}

#[derive(Debug)]
pub enum ProbeError {
    Transport(tonic::transport::Error),
    GrpcUnavailable(tonic::Status),
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(err) => write!(f, "transport: {err}"),
            Self::GrpcUnavailable(status) => write!(f, "grpc_unavailable: {status}"),
        }
    }
}

impl std::error::Error for ProbeError {}

impl From<tonic::transport::Error> for ProbeError {
    fn from(value: tonic::transport::Error) -> Self {
        Self::Transport(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::sync::{mpsc, oneshot};

    use crate::{
        reconcile::{ReconcileHandle, ReconcileRequest},
        xray::proto::xray::app::stats::command::stats_service_server::{
            StatsService, StatsServiceServer,
        },
    };

    #[test]
    fn xray_status_as_str_is_stable() {
        assert_eq!(XrayStatus::Unknown.as_str(), "unknown");
        assert_eq!(XrayStatus::Up.as_str(), "up");
        assert_eq!(XrayStatus::Down.as_str(), "down");
    }

    #[tokio::test]
    async fn down_to_up_triggers_full_reconcile_and_updates_snapshot() {
        // Pick an unused local port (best-effort).
        let tmp_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tmp_listener.local_addr().unwrap();
        drop(tmp_listener);

        let (tx, mut rx) = mpsc::unbounded_channel::<ReconcileRequest>();
        let reconcile = ReconcileHandle::from_sender(tx);

        let opts = XraySupervisorOptions {
            interval: Duration::from_millis(20),
            fails_before_down: 2,
            connect_timeout: Duration::from_millis(50),
            request_timeout: Duration::from_millis(50),
            down_log_throttle: Duration::from_secs(3600),
        };

        let (health, task) = spawn_xray_supervisor_with_options(addr, opts, reconcile);

        // Wait until marked Down.
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let snap = health.snapshot().await;
                if snap.status == XrayStatus::Down {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        // Bring up a minimal gRPC StatsService endpoint.
        #[derive(Debug, Default)]
        struct TestStats;

        #[tonic::async_trait]
        impl StatsService for TestStats {
            async fn get_stats(
                &self,
                _request: tonic::Request<GetStatsRequest>,
            ) -> Result<
                tonic::Response<crate::xray::proto::xray::app::stats::command::GetStatsResponse>,
                tonic::Status,
            > {
                Err(tonic::Status::not_found("missing stat"))
            }

            async fn get_stats_online(
                &self,
                _request: tonic::Request<GetStatsRequest>,
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
                _request: tonic::Request<GetStatsRequest>,
            ) -> Result<
                tonic::Response<
                    crate::xray::proto::xray::app::stats::command::GetStatsOnlineIpListResponse,
                >,
                tonic::Status,
            > {
                Err(tonic::Status::unimplemented("get_stats_online_ip_list"))
            }
        }

        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let server = tonic::transport::Server::builder()
            .add_service(StatsServiceServer::new(TestStats::default()))
            .serve_with_incoming_shutdown(incoming, async {
                let _ = shutdown_rx.await;
            });
        let server_handle = tokio::spawn(server);

        // Expect a Full reconcile request after the down -> up edge.
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Some(req) = rx.recv().await {
                    if req == ReconcileRequest::Full {
                        break;
                    }
                }
            }
        })
        .await
        .unwrap();

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let snap = health.snapshot().await;
                if snap.status == XrayStatus::Up && snap.recoveries_observed >= 1 {
                    assert!(snap.down_since.is_none());
                    assert!(snap.last_ok_at.is_some());
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        let _ = shutdown_tx.send(());
        let _ = server_handle.await;

        task.abort();
    }
}
