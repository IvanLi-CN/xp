use std::{future::Future, net::SocketAddr, pin::Pin, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use tokio::{
    sync::RwLock,
    time::{Instant, MissedTickBehavior},
};
use tracing::{debug, info, warn};

use crate::{
    config::{Config, XrayRestartMode},
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
    pub restart_attempts: u64,
    pub last_restart_at: Option<DateTime<Utc>>,
    pub last_restart_fail_at: Option<DateTime<Utc>>,
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
            restart_attempts: 0,
            last_restart_at: None,
            last_restart_fail_at: None,
        }
    }
}

pub type RestartFuture = Pin<Box<dyn Future<Output = Result<(), RestartError>> + Send>>;

pub trait XrayRestarter: Send + Sync {
    fn restart(&self) -> RestartFuture;
    fn name(&self) -> &'static str;
}

#[derive(Debug, Clone)]
struct SystemdRestarter {
    unit: String,
    timeout: Duration,
}

impl XrayRestarter for SystemdRestarter {
    fn restart(&self) -> RestartFuture {
        let unit = self.unit.clone();
        let timeout = self.timeout;
        Box::pin(async move { restart_systemd(&unit, timeout).await })
    }

    fn name(&self) -> &'static str {
        "systemd"
    }
}

#[derive(Debug, Clone)]
struct OpenrcRestarter {
    service: String,
    timeout: Duration,
}

impl XrayRestarter for OpenrcRestarter {
    fn restart(&self) -> RestartFuture {
        let service = self.service.clone();
        let timeout = self.timeout;
        Box::pin(async move { restart_openrc(&service, timeout).await })
    }

    fn name(&self) -> &'static str {
        "openrc"
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
    pub restart_cooldown: Duration,
}

impl XraySupervisorOptions {
    pub fn from_config(config: &Config) -> Self {
        Self {
            interval: Duration::from_secs(config.xray_health_interval_secs),
            fails_before_down: config.xray_health_fails_before_down as u32,
            connect_timeout: Duration::from_millis(500),
            request_timeout: Duration::from_millis(500),
            down_log_throttle: Duration::from_secs(30),
            restart_cooldown: Duration::from_secs(config.xray_restart_cooldown_secs),
        }
    }
}

pub fn spawn_xray_supervisor(
    config: Arc<Config>,
    reconcile: ReconcileHandle,
) -> (XrayHealthHandle, tokio::task::JoinHandle<()>) {
    let opts = XraySupervisorOptions::from_config(&config);
    let restarter = restarter_from_config(&config);
    spawn_xray_supervisor_with_options_and_restarter(
        config.xray_api_addr,
        opts,
        reconcile,
        restarter,
    )
}

pub fn spawn_xray_supervisor_with_options(
    xray_api_addr: SocketAddr,
    opts: XraySupervisorOptions,
    reconcile: ReconcileHandle,
) -> (XrayHealthHandle, tokio::task::JoinHandle<()>) {
    spawn_xray_supervisor_with_options_and_restarter(xray_api_addr, opts, reconcile, None)
}

pub fn spawn_xray_supervisor_with_options_and_restarter(
    xray_api_addr: SocketAddr,
    opts: XraySupervisorOptions,
    reconcile: ReconcileHandle,
    restarter: Option<Arc<dyn XrayRestarter>>,
) -> (XrayHealthHandle, tokio::task::JoinHandle<()>) {
    let health = XrayHealthHandle::new_unknown();
    let health_clone = health.clone();

    let task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(opts.interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        // Avoid log spam while Down by throttling periodic warnings.
        let mut last_down_warn_at: Option<Instant> = None;
        let mut last_restart_attempt_at: Option<Instant> = None;

        loop {
            interval.tick().await;

            let now = Utc::now();
            let probe =
                probe_xray_grpc(xray_api_addr, opts.connect_timeout, opts.request_timeout).await;

            let mut request_full = false;
            let mut restart_due = false;
            let mut restart_trigger = None::<&'static str>;

            {
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
                            request_full = true;
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
                            restart_trigger = Some("status_transition");
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

            if request_full {
                reconcile.request_full();
            }

            if restarter.is_some() {
                let now_i = Instant::now();
                let can_restart = last_restart_attempt_at
                    .map(|t| now_i.duration_since(t) >= opts.restart_cooldown)
                    .unwrap_or(true);
                if can_restart {
                    let snap = health_clone.snapshot().await;
                    if snap.status == XrayStatus::Down {
                        restart_due = true;
                        if restart_trigger.is_none() {
                            restart_trigger = Some("still_down");
                        }
                        last_restart_attempt_at = Some(now_i);
                    }
                }
            }

            if restart_due && let Some(restarter) = restarter.as_ref() {
                let attempt_at = Utc::now();
                let result = restarter.restart().await;

                let mut snap = health_clone.inner.write().await;
                snap.restart_attempts = snap.restart_attempts.saturating_add(1);
                snap.last_restart_at = Some(attempt_at);
                match result {
                    Ok(()) => {
                        info!(
                            restarter = restarter.name(),
                            trigger = restart_trigger.unwrap_or("unknown"),
                            "requested xray restart"
                        );
                    }
                    Err(err) => {
                        snap.last_restart_fail_at = Some(attempt_at);
                        warn!(
                            restarter = restarter.name(),
                            trigger = restart_trigger.unwrap_or("unknown"),
                            error = %err,
                            "failed to request xray restart"
                        );
                    }
                }
            }
        }
    });

    (health, task)
}

fn restarter_from_config(config: &Config) -> Option<Arc<dyn XrayRestarter>> {
    let timeout = Duration::from_secs(config.xray_restart_timeout_secs);
    match config.xray_restart_mode {
        XrayRestartMode::None => None,
        XrayRestartMode::Systemd => Some(Arc::new(SystemdRestarter {
            unit: config.xray_systemd_unit.clone(),
            timeout,
        })),
        XrayRestartMode::Openrc => Some(Arc::new(OpenrcRestarter {
            service: config.xray_openrc_service.clone(),
            timeout,
        })),
    }
}

async fn restart_systemd(unit: &str, timeout: Duration) -> Result<(), RestartError> {
    let args = ["restart", unit];
    run_command_with_timeout(
        &["/usr/bin/systemctl", "/bin/systemctl", "systemctl"],
        &args,
        timeout,
    )
    .await
    .map_err(|e| RestartError::Command {
        program: "systemctl",
        details: e,
    })
}

async fn restart_openrc(service: &str, timeout: Duration) -> Result<(), RestartError> {
    // Prefer doas on Alpine/OpenRC; fall back to sudo if doas is unavailable.
    let args_doas = ["-n", "/sbin/rc-service", service, "restart"];
    if let Ok(()) =
        run_command_with_timeout(&["/usr/bin/doas", "/bin/doas", "doas"], &args_doas, timeout).await
    {
        return Ok(());
    }
    let args_sudo = ["-n", "/sbin/rc-service", service, "restart"];
    run_command_with_timeout(&["/usr/bin/sudo", "/bin/sudo", "sudo"], &args_sudo, timeout)
        .await
        .map_err(|e| RestartError::Command {
            program: "doas/sudo",
            details: e,
        })
}

async fn run_command_with_timeout(
    programs: &[&str],
    args: &[&str],
    timeout: Duration,
) -> Result<(), String> {
    for program in programs {
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args);
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());

        let status = match tokio::time::timeout(timeout, cmd.status()).await {
            Ok(Ok(status)) => status,
            Ok(Err(err)) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Ok(Err(err)) => return Err(format!("spawn {program}: {err}")),
            Err(_) => return Err(format!("timeout running {program}")),
        };

        if status.success() {
            return Ok(());
        }
        return Err(format!("{program} exited with {status}"));
    }
    Err("no matching program found".to_string())
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

#[derive(Debug)]
pub enum RestartError {
    Command {
        program: &'static str,
        details: String,
    },
}

impl std::fmt::Display for RestartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Command { program, details } => write!(f, "{program}: {details}"),
        }
    }
}

impl std::error::Error for RestartError {}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};

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
            restart_cooldown: Duration::from_secs(3600),
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

    #[derive(Debug)]
    struct RecordingRestarter {
        calls: Arc<AtomicUsize>,
    }

    impl XrayRestarter for RecordingRestarter {
        fn restart(&self) -> RestartFuture {
            let calls = self.calls.clone();
            Box::pin(async move {
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
        }

        fn name(&self) -> &'static str {
            "test"
        }
    }

    #[tokio::test]
    async fn restart_is_throttled_by_cooldown_while_down() {
        let tmp_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tmp_listener.local_addr().unwrap();
        drop(tmp_listener);

        let (tx, _rx) = mpsc::unbounded_channel::<ReconcileRequest>();
        let reconcile = ReconcileHandle::from_sender(tx);

        let calls = Arc::new(AtomicUsize::new(0));
        let restarter: Arc<dyn XrayRestarter> = Arc::new(RecordingRestarter {
            calls: calls.clone(),
        });

        let opts = XraySupervisorOptions {
            interval: Duration::from_millis(20),
            fails_before_down: 1,
            connect_timeout: Duration::from_millis(50),
            request_timeout: Duration::from_millis(50),
            down_log_throttle: Duration::from_secs(3600),
            restart_cooldown: Duration::from_secs(3600),
        };

        let (_health, task) = spawn_xray_supervisor_with_options_and_restarter(
            addr,
            opts,
            reconcile,
            Some(restarter),
        );

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if calls.load(Ordering::Relaxed) >= 1 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(calls.load(Ordering::Relaxed), 1);

        task.abort();
    }
}
