use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use tokio::{
    sync::RwLock,
    time::{Instant, MissedTickBehavior},
};
use tracing::{debug, info, warn};

use crate::config::{Config, XrayRestartMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudflaredStatus {
    Disabled,
    Unknown,
    Up,
    Down,
}

impl CloudflaredStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Unknown => "unknown",
            Self::Up => "up",
            Self::Down => "down",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CloudflaredHealthSnapshot {
    pub status: CloudflaredStatus,
    pub last_ok_at: Option<DateTime<Utc>>,
    pub last_fail_at: Option<DateTime<Utc>>,
    pub down_since: Option<DateTime<Utc>>,
    pub consecutive_failures: u32,
    pub recoveries_observed: u64,
    pub restart_attempts: u64,
    pub last_restart_at: Option<DateTime<Utc>>,
    pub last_restart_fail_at: Option<DateTime<Utc>>,
}

impl Default for CloudflaredHealthSnapshot {
    fn default() -> Self {
        Self {
            status: CloudflaredStatus::Unknown,
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
pub type ProbeFuture = Pin<Box<dyn Future<Output = Result<(), ProbeError>> + Send>>;

pub trait CloudflaredRestarter: Send + Sync {
    fn restart(&self) -> RestartFuture;
    fn name(&self) -> &'static str;
}

pub trait CloudflaredProbe: Send + Sync {
    fn probe(&self, timeout: Duration) -> ProbeFuture;
}

#[derive(Debug, Clone)]
struct SystemdRestarter {
    unit: String,
    timeout: Duration,
}

impl CloudflaredRestarter for SystemdRestarter {
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

impl CloudflaredRestarter for OpenrcRestarter {
    fn restart(&self) -> RestartFuture {
        let service = self.service.clone();
        let timeout = self.timeout;
        Box::pin(async move { restart_openrc(&service, timeout).await })
    }

    fn name(&self) -> &'static str {
        "openrc"
    }
}

#[derive(Debug, Clone)]
struct SystemdProbe {
    unit: String,
}

impl CloudflaredProbe for SystemdProbe {
    fn probe(&self, timeout: Duration) -> ProbeFuture {
        let unit = self.unit.clone();
        Box::pin(async move {
            run_command_with_timeout(
                &["/usr/bin/systemctl", "/bin/systemctl", "systemctl"],
                &["is-active", "--quiet", &unit],
                timeout,
            )
            .await
            .map_err(|details| ProbeError::Command {
                program: "systemctl",
                details,
            })
        })
    }
}

#[derive(Debug, Clone)]
struct OpenrcProbe {
    service: String,
}

impl CloudflaredProbe for OpenrcProbe {
    fn probe(&self, timeout: Duration) -> ProbeFuture {
        let service = self.service.clone();
        Box::pin(async move {
            run_command_with_timeout(
                &["/sbin/rc-service", "/usr/sbin/rc-service", "rc-service"],
                &[&service, "status"],
                timeout,
            )
            .await
            .map_err(|details| ProbeError::Command {
                program: "rc-service",
                details,
            })
        })
    }
}

#[derive(Clone)]
pub struct CloudflaredHealthHandle {
    inner: Arc<RwLock<CloudflaredHealthSnapshot>>,
}

impl CloudflaredHealthHandle {
    pub fn new_with_status(status: CloudflaredStatus) -> Self {
        Self {
            inner: Arc::new(RwLock::new(CloudflaredHealthSnapshot {
                status,
                ..CloudflaredHealthSnapshot::default()
            })),
        }
    }

    pub async fn snapshot(&self) -> CloudflaredHealthSnapshot {
        self.inner.read().await.clone()
    }
}

#[derive(Debug, Clone)]
pub struct CloudflaredSupervisorOptions {
    pub interval: Duration,
    pub fails_before_down: u32,
    pub status_timeout: Duration,
    pub down_log_throttle: Duration,
    pub restart_cooldown: Duration,
}

impl CloudflaredSupervisorOptions {
    pub fn from_config(config: &Config) -> Self {
        Self {
            interval: Duration::from_secs(config.cloudflared_health_interval_secs),
            fails_before_down: config.cloudflared_health_fails_before_down as u32,
            status_timeout: Duration::from_millis(800),
            down_log_throttle: Duration::from_secs(30),
            restart_cooldown: Duration::from_secs(config.cloudflared_restart_cooldown_secs),
        }
    }
}

pub fn spawn_cloudflared_supervisor(
    config: Arc<Config>,
) -> (CloudflaredHealthHandle, tokio::task::JoinHandle<()>) {
    let opts = CloudflaredSupervisorOptions::from_config(&config);

    let mode = config.cloudflared_restart_mode;
    if mode == XrayRestartMode::None {
        let health = CloudflaredHealthHandle::new_with_status(CloudflaredStatus::Disabled);
        let task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(opts.interval);
            interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
            }
        });
        return (health, task);
    }

    let restarter = restarter_from_config(&config);
    let probe = probe_from_config(&config);
    spawn_cloudflared_supervisor_with_options_and_probe_and_restarter(opts, probe, restarter)
}

pub fn spawn_cloudflared_supervisor_with_options_and_probe_and_restarter(
    opts: CloudflaredSupervisorOptions,
    probe: Arc<dyn CloudflaredProbe>,
    restarter: Option<Arc<dyn CloudflaredRestarter>>,
) -> (CloudflaredHealthHandle, tokio::task::JoinHandle<()>) {
    let health = CloudflaredHealthHandle::new_with_status(CloudflaredStatus::Unknown);
    let health_clone = health.clone();

    let task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(opts.interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        let mut last_down_warn_at: Option<Instant> = None;
        let mut last_restart_attempt_at: Option<Instant> = None;

        loop {
            interval.tick().await;
            let now = Utc::now();

            let probe_out = probe.probe(opts.status_timeout).await;
            let mut restart_due = false;
            let mut restart_trigger = None::<&'static str>;

            {
                let mut snap = health_clone.inner.write().await;
                let prev = snap.status;

                match probe_out {
                    Ok(()) => {
                        snap.last_ok_at = Some(now);
                        snap.consecutive_failures = 0;

                        if prev == CloudflaredStatus::Down {
                            snap.status = CloudflaredStatus::Up;
                            snap.down_since = None;
                            snap.recoveries_observed = snap.recoveries_observed.saturating_add(1);
                            info!(
                                cloudflared_status = snap.status.as_str(),
                                recoveries_observed = snap.recoveries_observed,
                                "cloudflared recovered (down -> up)"
                            );
                        } else if prev != CloudflaredStatus::Up {
                            snap.status = CloudflaredStatus::Up;
                            info!(
                                cloudflared_status = snap.status.as_str(),
                                "cloudflared became available"
                            );
                        } else {
                            debug!(
                                cloudflared_status = snap.status.as_str(),
                                "cloudflared probe ok"
                            );
                        }

                        last_down_warn_at = None;
                    }
                    Err(err) => {
                        snap.last_fail_at = Some(now);
                        snap.consecutive_failures = snap.consecutive_failures.saturating_add(1);

                        let should_mark_down = snap.consecutive_failures >= opts.fails_before_down
                            && prev != CloudflaredStatus::Down;

                        if should_mark_down {
                            snap.status = CloudflaredStatus::Down;
                            snap.down_since = Some(now);
                            warn!(
                                cloudflared_status = snap.status.as_str(),
                                consecutive_failures = snap.consecutive_failures,
                                error = %err,
                                "cloudflared marked down"
                            );
                            last_down_warn_at = Some(Instant::now());
                            restart_trigger = Some("status_transition");
                        }

                        if snap.status == CloudflaredStatus::Down {
                            let now_i = Instant::now();
                            let should_warn = last_down_warn_at
                                .map(|t| now_i.duration_since(t) >= opts.down_log_throttle)
                                .unwrap_or(true);
                            if should_warn {
                                warn!(
                                    cloudflared_status = snap.status.as_str(),
                                    consecutive_failures = snap.consecutive_failures,
                                    error = %err,
                                    "cloudflared still down"
                                );
                                last_down_warn_at = Some(now_i);
                            } else {
                                debug!(
                                    cloudflared_status = snap.status.as_str(),
                                    consecutive_failures = snap.consecutive_failures,
                                    error = %err,
                                    "cloudflared probe failed (throttled)"
                                );
                            }
                        } else if prev != CloudflaredStatus::Unknown {
                            debug!(
                                cloudflared_status = snap.status.as_str(),
                                consecutive_failures = snap.consecutive_failures,
                                error = %err,
                                "cloudflared probe failed"
                            );
                        } else {
                            debug!(
                                cloudflared_status = snap.status.as_str(),
                                consecutive_failures = snap.consecutive_failures,
                                error = %err,
                                "cloudflared probe failed (startup)"
                            );
                        }
                    }
                }
            }

            if restarter.is_some() {
                let now_i = Instant::now();
                let can_restart = last_restart_attempt_at
                    .map(|t| now_i.duration_since(t) >= opts.restart_cooldown)
                    .unwrap_or(true);
                if can_restart {
                    let snap = health_clone.snapshot().await;
                    if snap.status == CloudflaredStatus::Down {
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
                            "requested cloudflared restart"
                        );
                    }
                    Err(err) => {
                        snap.last_restart_fail_at = Some(attempt_at);
                        warn!(
                            restarter = restarter.name(),
                            trigger = restart_trigger.unwrap_or("unknown"),
                            error = %err,
                            "failed to request cloudflared restart"
                        );
                    }
                }
            }
        }
    });

    (health, task)
}

fn probe_from_config(config: &Config) -> Arc<dyn CloudflaredProbe> {
    match config.cloudflared_restart_mode {
        XrayRestartMode::None | XrayRestartMode::Systemd => Arc::new(SystemdProbe {
            unit: config.cloudflared_systemd_unit.clone(),
        }),
        XrayRestartMode::Openrc => Arc::new(OpenrcProbe {
            service: config.cloudflared_openrc_service.clone(),
        }),
    }
}

fn restarter_from_config(config: &Config) -> Option<Arc<dyn CloudflaredRestarter>> {
    let timeout = Duration::from_secs(config.cloudflared_restart_timeout_secs);
    match config.cloudflared_restart_mode {
        XrayRestartMode::None => None,
        XrayRestartMode::Systemd => Some(Arc::new(SystemdRestarter {
            unit: config.cloudflared_systemd_unit.clone(),
            timeout,
        })),
        XrayRestartMode::Openrc => Some(Arc::new(OpenrcRestarter {
            service: config.cloudflared_openrc_service.clone(),
            timeout,
        })),
    }
}

async fn restart_systemd(unit: &str, timeout: Duration) -> Result<(), RestartError> {
    run_command_with_timeout(
        &["/usr/bin/systemctl", "/bin/systemctl", "systemctl"],
        &["restart", unit],
        timeout,
    )
    .await
    .map_err(|details| RestartError::Command {
        program: "systemctl",
        details,
    })
}

async fn restart_openrc(service: &str, timeout: Duration) -> Result<(), RestartError> {
    let args_doas = ["-n", "/sbin/rc-service", service, "restart"];
    if let Ok(()) =
        run_command_with_timeout(&["/usr/bin/doas", "/bin/doas", "doas"], &args_doas, timeout).await
    {
        return Ok(());
    }

    let args_sudo = ["-n", "/sbin/rc-service", service, "restart"];
    run_command_with_timeout(&["/usr/bin/sudo", "/bin/sudo", "sudo"], &args_sudo, timeout)
        .await
        .map_err(|details| RestartError::Command {
            program: "doas/sudo",
            details,
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

#[derive(Debug)]
pub enum ProbeError {
    Command {
        program: &'static str,
        details: String,
    },
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Command { program, details } => write!(f, "{program}: {details}"),
        }
    }
}

impl std::error::Error for ProbeError {}

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

    struct AlwaysDownProbe;

    impl CloudflaredProbe for AlwaysDownProbe {
        fn probe(&self, _timeout: Duration) -> ProbeFuture {
            Box::pin(async {
                Err(ProbeError::Command {
                    program: "test",
                    details: "down".to_string(),
                })
            })
        }
    }

    struct RecordingRestarter {
        calls: Arc<AtomicUsize>,
    }

    impl CloudflaredRestarter for RecordingRestarter {
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

    #[test]
    fn cloudflared_status_as_str_is_stable() {
        assert_eq!(CloudflaredStatus::Disabled.as_str(), "disabled");
        assert_eq!(CloudflaredStatus::Unknown.as_str(), "unknown");
        assert_eq!(CloudflaredStatus::Up.as_str(), "up");
        assert_eq!(CloudflaredStatus::Down.as_str(), "down");
    }

    #[tokio::test]
    async fn restart_is_throttled_by_cooldown_while_down() {
        let calls = Arc::new(AtomicUsize::new(0));
        let restarter: Arc<dyn CloudflaredRestarter> = Arc::new(RecordingRestarter {
            calls: calls.clone(),
        });

        let opts = CloudflaredSupervisorOptions {
            interval: Duration::from_millis(20),
            fails_before_down: 1,
            status_timeout: Duration::from_millis(20),
            down_log_throttle: Duration::from_secs(3600),
            restart_cooldown: Duration::from_secs(3600),
        };

        let (_health, task) = spawn_cloudflared_supervisor_with_options_and_probe_and_restarter(
            opts,
            Arc::new(AlwaysDownProbe),
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
