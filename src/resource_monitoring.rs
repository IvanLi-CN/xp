use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    path::Path,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::RwLock,
    time::{MissedTickBehavior, interval, timeout},
};
use tracing::warn;

mod resource_monitoring_collector;
mod resource_monitoring_logic;
mod resource_monitoring_policy;
mod resource_monitoring_store;
mod resource_monitoring_wire;
pub(crate) use resource_monitoring_collector::unsupported_snapshot;
use resource_monitoring_collector::{CollectorState, LinuxResourceReader};
use resource_monitoring_logic::{
    AlertProgress, auto_history_resolution, extract_point, merge_pending_gap,
    resource_capture_alert,
};
pub use resource_monitoring_policy::{ResourcePolicy, ResourcePolicyOverride};
use resource_monitoring_store::ResourceStore;
pub use resource_monitoring_store::{ResourceCapacityError, resource_history_capacity_preflight};

pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(15);
pub const MAX_SAMPLES: usize = 240;
pub const RESOURCE_HISTORY_SCHEMA: &str = "resource_metrics.v1";
pub const RESOURCE_HISTORY_STREAM: &str = "resource_metrics-v1";
pub const RESOURCE_MINUTE_PAYLOAD_LIMIT: usize = 2 * 1024;
pub const RESOURCE_15_MINUTE_PAYLOAD_LIMIT: usize = 1024;
pub const RESOURCE_HOUR_PAYLOAD_LIMIT: usize = 768;
pub const RESOURCE_HISTORY_PER_NODE_CAPACITY_BYTES: u64 = 82 * 1024 * 1024;
pub const RESOURCE_HISTORY_MAX_QUOTA_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const RESOURCE_SAMPLE_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_LOCAL_ROLLUPS: usize = 1_440;
const MAX_ALERT_TRANSITIONS: usize = 1_000;
const RESOURCE_MINUTE_WINDOW_SECONDS: i64 = 14 * 24 * 60 * 60;
const RESOURCE_15_MINUTE_WINDOW_SECONDS: i64 = 104 * 24 * 60 * 60;

const DOMAIN_HISTORY_METRICS: [&str; 11] = [
    "cpu_busy_percent",
    "cpu_iowait_percent",
    "load1",
    "memory_available_bytes",
    "memory_total_bytes",
    "swap_total_bytes",
    "swap_free_bytes",
    "filesystem.root.used_percent",
    "filesystem.data.used_percent",
    "filesystem.root.used_inode_percent",
    "filesystem.data.used_inode_percent",
];
const RUNTIME_HISTORY_METRICS: [&str; 7] = [
    "cpu_percent",
    "rss_bytes",
    "pss_bytes",
    "read_bytes_per_second",
    "write_bytes_per_second",
    "fd_count",
    "thread_count",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceDomain {
    Host,
    Cgroup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceRole {
    Xp,
    Xray,
    Cloudflared,
    Canary,
}

impl ResourceRole {
    pub const ALL: [Self; 4] = [Self::Xp, Self::Xray, Self::Cloudflared, Self::Canary];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Xp => "xp",
            Self::Xray => "xray",
            Self::Cloudflared => "cloudflared",
            Self::Canary => "canary",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    #[default]
    Supported,
    Partial,
    Unsupported,
}

impl Capability {
    fn worst(self, other: Self) -> Self {
        self.max(other)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Measurement<T> {
    pub capability: Capability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
}

impl<T> Measurement<T> {
    pub fn supported(value: T) -> Self {
        Self {
            capability: Capability::Supported,
            value: Some(value),
            reason_code: None,
        }
    }

    pub fn unsupported(reason: impl Into<String>) -> Self {
        Self {
            capability: Capability::Unsupported,
            value: None,
            reason_code: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemMetrics {
    pub mount: String,
    pub capability: Capability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_inodes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_inodes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_inode_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainMetrics {
    pub cpu_busy_percent: Measurement<f64>,
    pub cpu_iowait_percent: Measurement<f64>,
    pub load1: Measurement<f64>,
    pub memory_total_bytes: Measurement<u64>,
    pub memory_available_bytes: Measurement<u64>,
    pub swap_total_bytes: Measurement<u64>,
    pub swap_free_bytes: Measurement<u64>,
    pub filesystems: Vec<FilesystemMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeMetrics {
    pub cpu_percent: Measurement<f64>,
    pub rss_bytes: Measurement<u64>,
    pub pss_bytes: Measurement<u64>,
    pub read_bytes_per_second: Measurement<f64>,
    pub write_bytes_per_second: Measurement<f64>,
    pub fd_count: Measurement<u64>,
    pub thread_count: Measurement<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    pub role: ResourceRole,
    pub state: String,
    pub capability: Capability,
    pub metrics: RuntimeMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSnapshot {
    pub node_id: String,
    pub observed_at: String,
    pub resource_domain: ResourceDomain,
    pub capture_state: String,
    pub capability: Capability,
    pub domain: DomainMetrics,
    pub runtimes: Vec<RuntimeSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSeriesPoint {
    pub observed_at: String,
    pub value: Option<f64>,
    pub capability: Capability,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRecentSeries {
    pub metric: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<ResourceRole>,
    pub resolution: String,
    pub points: Vec<ResourceSeriesPoint>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRollup {
    pub node_id: String,
    pub bucket_start_unix_seconds: i64,
    pub expected_samples: u32,
    pub captured_samples: u32,
    pub capability: Capability,
    pub values: BTreeMap<String, RollupValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollupValue {
    pub min: Option<f64>,
    pub mean: Option<f64>,
    pub max: Option<f64>,
    pub last: Option<f64>,
    pub counter_delta: Option<f64>,
    pub capability: Capability,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceGap {
    pub from_bucket_start_unix_seconds: i64,
    pub to_bucket_start_unix_seconds: i64,
    pub reason_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceHistoryResponse {
    pub metric: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<ResourceRole>,
    pub resolution: String,
    pub quality: String,
    pub coverage: Option<(i64, i64)>,
    pub watermark: Option<i64>,
    pub gaps: Vec<ResourceGap>,
    pub freshness_seconds: Option<i64>,
    pub truncated: bool,
    pub points: Vec<ResourceSeriesPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceAlert {
    pub id: String,
    pub alert_type: String,
    pub node_id: String,
    pub scope: String,
    pub metric: String,
    pub severity: String,
    pub opened_at: String,
    pub latest_bucket_start_unix_seconds: i64,
}

#[derive(Debug, Clone)]
pub enum ResourceHistoryPayload {
    Rollup {
        resolution: String,
        rollup: ResourceRollup,
    },
    CaptureGap {
        resolution: String,
        gap: ResourceGap,
    },
}

impl ResourceHistoryPayload {
    pub fn encoded_size(&self) -> usize {
        serde_json::to_vec(self).map_or(usize::MAX, |payload| payload.len())
    }

    pub fn validate_budget(&self) -> Result<(), &'static str> {
        let limit = match self {
            Self::Rollup { resolution, .. } | Self::CaptureGap { resolution, .. } => {
                match resolution.as_str() {
                    "1m" => RESOURCE_MINUTE_PAYLOAD_LIMIT,
                    "15m" => RESOURCE_15_MINUTE_PAYLOAD_LIMIT,
                    "1h" => RESOURCE_HOUR_PAYLOAD_LIMIT,
                    _ => return Err("resource_resolution_unsupported"),
                }
            }
        };
        (self.encoded_size() <= limit)
            .then_some(())
            .ok_or("resource_payload_budget_exceeded")
    }
}

struct RollupAccumulator {
    bucket: Option<i64>,
    expected_samples: u32,
    captured_samples: u32,
    values: BTreeMap<String, RollupAccumulatorValue>,
}

#[derive(Debug, Default)]
struct RollupAccumulatorValue {
    min: Option<f64>,
    sum: f64,
    max: Option<f64>,
    last: Option<f64>,
    count: u32,
    counter_sum: f64,
    counter_reset: bool,
    capability: Capability,
}

impl RollupAccumulator {
    fn new() -> Self {
        Self {
            bucket: None,
            expected_samples: 4,
            captured_samples: 0,
            values: BTreeMap::new(),
        }
    }

    fn add(&mut self, snapshot: &ResourceSnapshot, bucket: i64) -> Option<ResourceRollup> {
        let completed = if self.bucket.is_some() && self.bucket != Some(bucket) {
            self.finish()
        } else {
            None
        };
        if self.bucket != Some(bucket) {
            self.bucket = Some(bucket);
            self.expected_samples = 4;
            self.captured_samples = 0;
            self.values.clear();
        }
        self.captured_samples = self.captured_samples.saturating_add(1);
        self.record(
            "domain.cpu_busy_percent",
            snapshot.domain.cpu_busy_percent.value,
            snapshot.domain.cpu_busy_percent.capability,
        );
        self.record(
            "domain.cpu_iowait_percent",
            snapshot.domain.cpu_iowait_percent.value,
            snapshot.domain.cpu_iowait_percent.capability,
        );
        self.record(
            "domain.memory_available_bytes",
            snapshot
                .domain
                .memory_available_bytes
                .value
                .map(|value| value as f64),
            snapshot.domain.memory_available_bytes.capability,
        );
        self.record(
            "domain.memory_total_bytes",
            snapshot
                .domain
                .memory_total_bytes
                .value
                .map(|value| value as f64),
            snapshot.domain.memory_total_bytes.capability,
        );
        self.record(
            "domain.load1",
            snapshot.domain.load1.value,
            snapshot.domain.load1.capability,
        );
        self.record(
            "domain.swap_total_bytes",
            snapshot
                .domain
                .swap_total_bytes
                .value
                .map(|value| value as f64),
            snapshot.domain.swap_total_bytes.capability,
        );
        self.record(
            "domain.swap_free_bytes",
            snapshot
                .domain
                .swap_free_bytes
                .value
                .map(|value| value as f64),
            snapshot.domain.swap_free_bytes.capability,
        );
        for filesystem in &snapshot.domain.filesystems {
            self.record(
                &format!("domain.filesystem.{}.used_percent", filesystem.mount),
                filesystem.used_percent,
                filesystem.capability,
            );
            self.record(
                &format!("domain.filesystem.{}.used_inode_percent", filesystem.mount),
                filesystem.used_inode_percent,
                filesystem.capability,
            );
        }
        for runtime in &snapshot.runtimes {
            self.record(
                &format!("{}.cpu_percent", runtime.role.as_str()),
                runtime.metrics.cpu_percent.value,
                runtime.metrics.cpu_percent.capability,
            );
            self.record(
                &format!("{}.rss_bytes", runtime.role.as_str()),
                runtime.metrics.rss_bytes.value.map(|value| value as f64),
                runtime.metrics.rss_bytes.capability,
            );
            self.record(
                &format!("{}.pss_bytes", runtime.role.as_str()),
                runtime.metrics.pss_bytes.value.map(|value| value as f64),
                runtime.metrics.pss_bytes.capability,
            );
            self.record(
                &format!("{}.read_bytes_per_second", runtime.role.as_str()),
                runtime.metrics.read_bytes_per_second.value,
                runtime.metrics.read_bytes_per_second.capability,
            );
            self.record(
                &format!("{}.write_bytes_per_second", runtime.role.as_str()),
                runtime.metrics.write_bytes_per_second.value,
                runtime.metrics.write_bytes_per_second.capability,
            );
            self.record(
                &format!("{}.fd_count", runtime.role.as_str()),
                runtime.metrics.fd_count.value.map(|value| value as f64),
                runtime.metrics.fd_count.capability,
            );
            self.record(
                &format!("{}.thread_count", runtime.role.as_str()),
                runtime.metrics.thread_count.value.map(|value| value as f64),
                runtime.metrics.thread_count.capability,
            );
        }
        completed
    }

    fn record(&mut self, key: &str, value: Option<f64>, capability: Capability) {
        let entry = self.values.entry(key.to_string()).or_default();
        entry.capability = entry.capability.worst(capability);
        let Some(value) = value else {
            if key.ends_with("read_bytes_per_second") || key.ends_with("write_bytes_per_second") {
                entry.counter_reset = true;
            }
            return;
        };
        entry.min = Some(entry.min.map_or(value, |current| current.min(value)));
        entry.max = Some(entry.max.map_or(value, |current| current.max(value)));
        entry.sum += value;
        if key.ends_with("read_bytes_per_second") || key.ends_with("write_bytes_per_second") {
            entry.counter_sum += value * SAMPLE_INTERVAL.as_secs_f64();
        }
        entry.last = Some(value);
        entry.count = entry.count.saturating_add(1);
    }

    fn finish(&mut self) -> Option<ResourceRollup> {
        let bucket = self.bucket?;
        let values = self
            .values
            .iter()
            .map(|(key, value)| {
                (
                    key.clone(),
                    RollupValue {
                        min: value.min,
                        mean: (value.count > 0).then(|| value.sum / f64::from(value.count)),
                        max: value.max,
                        last: value.last,
                        counter_delta: (!value.counter_reset && value.counter_sum > 0.0)
                            .then_some(value.counter_sum),
                        capability: value.capability,
                    },
                )
            })
            .collect();
        Some(ResourceRollup {
            node_id: String::new(),
            bucket_start_unix_seconds: bucket,
            expected_samples: self.expected_samples,
            captured_samples: self.captured_samples,
            capability: self
                .values
                .values()
                .fold(Capability::Supported, |current, value| {
                    current.worst(value.capability)
                }),
            values,
        })
    }
}

#[derive(Clone)]
pub struct ResourceMonitorHandle {
    inner: Arc<RwLock<ResourceState>>,
    store: Arc<StdMutex<ResourceStore>>,
    state_store: Option<Arc<tokio::sync::Mutex<crate::state::JsonSnapshotStore>>>,
}

struct ResourceState {
    node_id: String,
    reader: LinuxResourceReader,
    collector: CollectorState,
    samples: VecDeque<ResourceSnapshot>,
    current: Option<ResourceSnapshot>,
    rollup: RollupAccumulator,
    alert_progress: HashMap<String, AlertProgress>,
    pending_gap: Option<ResourceGap>,
}

enum ResourceAlertAction {
    Open(ResourceAlert),
    Recover(String),
}

struct ThresholdConfig {
    warning: f64,
    warning_minutes: u32,
    critical: f64,
    critical_minutes: u32,
}

impl ResourceMonitorHandle {
    pub fn start(data_dir: &Path, node_id: String) -> Self {
        Self::start_with_state(data_dir, node_id, None)
    }

    pub fn start_with_state(
        data_dir: &Path,
        node_id: String,
        state_store: Option<Arc<tokio::sync::Mutex<crate::state::JsonSnapshotStore>>>,
    ) -> Self {
        let store = ResourceStore::open(data_dir).unwrap_or_else(|error| {
            warn!(error = %error, "resource store unavailable; using memory-only monitoring");
            ResourceStore::memory()
        });
        let alert_progress = store
            .alerts()
            .unwrap_or_default()
            .into_iter()
            .map(|alert| {
                (
                    alert.metric,
                    AlertProgress {
                        severity: Some(alert.severity),
                        streak_minutes: 1,
                    },
                )
            })
            .collect();
        let handle = Self {
            inner: Arc::new(RwLock::new(ResourceState {
                node_id: node_id.clone(),
                reader: LinuxResourceReader::new(data_dir.to_path_buf()),
                collector: CollectorState::default(),
                samples: VecDeque::with_capacity(MAX_SAMPLES),
                current: None,
                rollup: RollupAccumulator::new(),
                alert_progress,
                pending_gap: None,
            })),
            store: Arc::new(StdMutex::new(store)),
            state_store,
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let worker = handle.clone();
            runtime.spawn(async move {
                let mut ticker = interval(SAMPLE_INTERVAL);
                ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
                loop {
                    ticker.tick().await;
                    worker.sample_once().await;
                }
            });
        }
        handle
    }

    pub async fn sample_once(&self) {
        let policy = self.effective_policy().await;
        let (reader, node_id, collector) = {
            let mut state = self.inner.write().await;
            (
                state.reader.clone(),
                state.node_id.clone(),
                std::mem::take(&mut state.collector),
            )
        };
        let read_node_id = node_id.clone();
        let read_result = timeout(
            RESOURCE_SAMPLE_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                let mut collector = collector;
                let snapshot = reader.read(&read_node_id, &mut collector);
                (snapshot, collector)
            }),
        )
        .await;
        let (mut snapshot, collector, timed_out) = match read_result {
            Ok(Ok((snapshot, collector))) => (snapshot, collector, false),
            Ok(Err(_)) | Err(_) => {
                let mut snapshot = unsupported_snapshot(&node_id);
                snapshot.capture_state = "suspended".to_string();
                (snapshot, CollectorState::default(), true)
            }
        };
        let (snapshot, rollup, mut alert_actions) = {
            let mut state = self.inner.write().await;
            if !timed_out {
                state.collector = collector;
            }
            if self
                .store
                .lock()
                .map(|store| !store.is_persistent())
                .unwrap_or(true)
            {
                snapshot.capture_state = "suspended".to_string();
            }
            if timed_out {
                let bucket = Utc::now().timestamp() / 60 * 60;
                state.pending_gap = Some(match state.pending_gap.take() {
                    Some(mut gap) => {
                        gap.to_bucket_start_unix_seconds = bucket;
                        gap
                    }
                    None => ResourceGap {
                        from_bucket_start_unix_seconds: bucket,
                        to_bucket_start_unix_seconds: bucket,
                        reason_code: "resource_sample_timeout".to_string(),
                    },
                });
            }
            let bucket = DateTime::parse_from_rfc3339(&snapshot.observed_at)
                .map(|value| value.timestamp() / 60 * 60)
                .unwrap_or_else(|_| Utc::now().timestamp() / 60 * 60);
            if let Some(previous_bucket) = state.rollup.bucket
                && bucket > previous_bucket + 60
            {
                let gap = ResourceGap {
                    from_bucket_start_unix_seconds: previous_bucket + 60,
                    to_bucket_start_unix_seconds: bucket - 60,
                    reason_code: "resource_sample_gap".to_string(),
                };
                state.pending_gap = Some(match state.pending_gap.take() {
                    Some(mut current) => {
                        current.to_bucket_start_unix_seconds = gap.to_bucket_start_unix_seconds;
                        current
                    }
                    None => gap,
                });
            }
            let rollup = state.rollup.add(&snapshot, bucket).map(|mut rollup| {
                rollup.node_id = state.node_id.clone();
                rollup
            });
            let alert_actions = rollup
                .as_ref()
                .map(|rollup| state.evaluate_alerts(rollup, &policy))
                .unwrap_or_default();
            state.samples.push_front(snapshot.clone());
            state.samples.truncate(MAX_SAMPLES);
            state.current = Some(snapshot.clone());
            (snapshot, rollup, alert_actions)
        };
        if let Some(rollup) = rollup {
            let save_result = self.store.lock().map_err(|_| ()).and_then(|mut store| {
                store
                    .is_persistent()
                    .then_some(())
                    .ok_or(())
                    .and_then(|()| store.save_rollup(&rollup).map_err(|_| ()))
            });
            if save_result.is_err() {
                let mut state = self.inner.write().await;
                let was_suspended = state
                    .current
                    .as_ref()
                    .is_some_and(|current| current.capture_state != "active");
                if let Some(current) = state.current.as_mut() {
                    current.capture_state = "suspended".to_string();
                }
                state.pending_gap = Some(match state.pending_gap.take() {
                    Some(mut gap) => {
                        gap.to_bucket_start_unix_seconds = rollup.bucket_start_unix_seconds;
                        gap
                    }
                    None => ResourceGap {
                        from_bucket_start_unix_seconds: rollup.bucket_start_unix_seconds,
                        to_bucket_start_unix_seconds: rollup.bucket_start_unix_seconds,
                        reason_code: "resource_store_unavailable".to_string(),
                    },
                });
                if !was_suspended {
                    alert_actions.push(ResourceAlertAction::Open(resource_capture_alert(
                        &rollup.node_id,
                        rollup.bucket_start_unix_seconds,
                    )));
                }
            } else {
                let gap = {
                    let mut state = self.inner.write().await;
                    let was_suspended = state
                        .current
                        .as_ref()
                        .is_some_and(|current| current.capture_state != "active");
                    if let Some(current) = state.current.as_mut() {
                        current.capture_state = "active".to_string();
                    }
                    if was_suspended {
                        alert_actions.push(ResourceAlertAction::Recover(format!(
                            "{}:capture",
                            rollup.node_id
                        )));
                    }
                    state.pending_gap.take()
                };
                if let Some(gap) = gap {
                    let save_failed = self
                        .store
                        .lock()
                        .map(|mut store| store.save_gap(&gap).is_err())
                        .unwrap_or(true);
                    if save_failed {
                        let mut state = self.inner.write().await;
                        state.pending_gap = Some(gap);
                    }
                }
            }
            if let Ok(mut store) = self.store.lock() {
                for action in alert_actions {
                    match action {
                        ResourceAlertAction::Open(alert) => {
                            let _ = store.save_alert(&alert);
                        }
                        ResourceAlertAction::Recover(id) => {
                            let _ = store.clear_alert(&id);
                        }
                    }
                }
            }
        }
        let _ = snapshot;
    }

    pub async fn current(&self) -> ResourceSnapshot {
        let state = self.inner.read().await;
        state
            .current
            .clone()
            .unwrap_or_else(|| unsupported_snapshot(&state.node_id))
    }

    pub async fn recent(&self, metric: &str, role: Option<ResourceRole>) -> ResourceRecentSeries {
        let samples = self.inner.read().await.samples.clone();
        let mut points = samples
            .into_iter()
            .filter_map(|sample| extract_point(&sample, metric, role))
            .collect::<Vec<_>>();
        points.reverse();
        ResourceRecentSeries {
            metric: metric.to_string(),
            role,
            resolution: "15s".to_string(),
            truncated: false,
            points,
        }
    }

    pub fn history(
        &self,
        metric: String,
        role: Option<ResourceRole>,
        limit: usize,
        from: Option<i64>,
        to: Option<i64>,
        resolution: Option<String>,
    ) -> ResourceHistoryResponse {
        let resolution = match resolution.as_deref() {
            Some("15m") => "15m",
            Some("1h") => "1h",
            Some("1m") => "1m",
            _ => auto_history_resolution(from, to),
        };
        let (points, mut gaps) = self
            .store
            .lock()
            .ok()
            .map(|store| {
                (
                    store
                        .history(&metric, role, limit, from, to, resolution)
                        .unwrap_or_default(),
                    store.gaps().unwrap_or_default(),
                )
            })
            .unwrap_or_default();
        merge_pending_gap(
            &mut gaps,
            self.inner
                .try_read()
                .ok()
                .and_then(|state| state.pending_gap.clone()),
        );
        let now = Utc::now().timestamp();
        ResourceHistoryResponse {
            metric,
            role,
            resolution: resolution.to_string(),
            quality: if !gaps.is_empty() {
                "partial".to_string()
            } else if points.is_empty() {
                "local_only".to_string()
            } else {
                "complete".to_string()
            },
            coverage: points
                .first()
                .and_then(|point| point.observed_at.parse::<DateTime<Utc>>().ok())
                .zip(
                    points
                        .last()
                        .and_then(|point| point.observed_at.parse::<DateTime<Utc>>().ok()),
                )
                .map(|(from, to)| (from.timestamp(), to.timestamp())),
            watermark: points
                .last()
                .and_then(|point| point.observed_at.parse::<DateTime<Utc>>().ok())
                .map(|time| time.timestamp()),
            gaps,
            freshness_seconds: points
                .last()
                .and_then(|point| point.observed_at.parse::<DateTime<Utc>>().ok())
                .map(|time| now.saturating_sub(time.timestamp())),
            truncated: points.len() >= limit,
            points,
        }
    }

    pub fn policy(&self) -> ResourcePolicy {
        self.store
            .lock()
            .ok()
            .and_then(|store| store.policy().ok())
            .unwrap_or_default()
    }

    pub async fn effective_policy(&self) -> ResourcePolicy {
        if let Some(state_store) = &self.state_store
            && let Some(policy) = state_store
                .lock()
                .await
                .state()
                .resource_policy
                .as_ref()
                .and_then(|value| serde_json::from_value(value.clone()).ok())
        {
            return policy;
        }
        self.policy()
    }

    pub fn sync_policy(&self, policy: &ResourcePolicy) -> Result<(), PolicyError> {
        self.store
            .lock()
            .map_err(|_| PolicyError::Store)?
            .save_policy(policy)
            .map_err(|_| PolicyError::Store)
    }

    pub fn update_policy(
        &self,
        expected_revision: u64,
        mut policy: ResourcePolicy,
    ) -> Result<ResourcePolicy, PolicyError> {
        let mut store = self.store.lock().map_err(|_| PolicyError::Store)?;
        let current = store.policy().map_err(|_| PolicyError::Store)?;
        if current.revision != expected_revision {
            return Err(PolicyError::Conflict {
                current_revision: current.revision,
            });
        }
        policy.revision = expected_revision.saturating_add(1);
        policy.validate().map_err(PolicyError::Invalid)?;
        store.save_policy(&policy).map_err(|_| PolicyError::Store)?;
        Ok(policy)
    }

    pub fn alerts(&self) -> Vec<ResourceAlert> {
        self.store
            .lock()
            .ok()
            .and_then(|store| store.alerts().ok())
            .unwrap_or_default()
    }

    pub fn pending_rollups(&self, limit: usize) -> Vec<ResourceRollup> {
        self.store
            .lock()
            .ok()
            .and_then(|store| store.pending_rollups(limit).ok())
            .unwrap_or_default()
    }

    pub fn mark_rollups_enqueued(&self, buckets: &[i64]) {
        if let Ok(mut store) = self.store.lock() {
            let _ = store.mark_rollups_enqueued(buckets);
        }
    }

    pub fn pending_gaps(&self, limit: usize) -> Vec<(i64, ResourceGap)> {
        self.store
            .lock()
            .ok()
            .and_then(|store| store.pending_gaps(limit).ok())
            .unwrap_or_default()
    }

    pub fn mark_gaps_enqueued(&self, ids: &[i64]) {
        if let Ok(mut store) = self.store.lock() {
            let _ = store.mark_gaps_enqueued(ids);
        }
    }
}

pub fn validate_history_metric(
    metric: &str,
    role: Option<ResourceRole>,
) -> Result<(), &'static str> {
    let valid = if role.is_some() {
        RUNTIME_HISTORY_METRICS.contains(&metric)
    } else {
        DOMAIN_HISTORY_METRICS.contains(&metric)
    };
    valid
        .then_some(())
        .ok_or("resource metric is not supported")
}

#[derive(Debug)]
pub enum PolicyError {
    Conflict { current_revision: u64 },
    Invalid(&'static str),
    Store,
}

#[cfg(test)]
#[path = "resource_monitoring/resource_monitoring_tests.rs"]
mod tests;
