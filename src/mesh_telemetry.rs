use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration as StdDuration, Instant},
};

use chrono::{DateTime, Duration, SecondsFormat, Timelike, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, Semaphore};
use tracing::warn;

use crate::state::history_storage::read_legacy_mesh_telemetry;

const TELEMETRY_SCHEMA_VERSION: u32 = 1;
const MAX_EVENTS: usize = 200;
const MAX_BUCKETS: usize = 24 * 60;
const SAMPLE_PERSIST_INTERVAL: StdDuration = StdDuration::from_secs(5);

mod reverse;
mod transport;
pub use reverse::ReverseRelayTelemetrySample;
use transport::MeshConnectionTrackers;
pub(crate) use transport::{MeshConnectionFingerprint, MeshTransportObservation};
pub use transport::{
    MeshTransportHealth, MeshTransportProtocol, mesh_transport_counts_for,
    mesh_transport_health_for,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryPath {
    Mesh,
    Public,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActiveRouteKind {
    RealityDirect,
    ReverseRelay,
    Public,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MeshActiveRoute {
    pub kind: ActiveRouteKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendezvous: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rendezvous_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_rendezvous: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standby_rendezvous: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readiness: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MeshQuality {
    Good,
    Slow,
    Unstable,
    Down,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BreakerState {
    Closed,
    Open,
    HalfOpen,
    Disabled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MeshPeerReason {
    MeshAvailable,
    MissingEndpoint,
    AmbiguousEndpoint,
    InvalidAccessHost,
    NoSample,
    TransportTimeout,
    TransportError,
    ProtocolRejected,
    FallbackActive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshTelemetryEvent {
    pub at: String,
    pub peer_id: String,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MeshTelemetryBucket {
    pub minute: String,
    pub mesh_success: u32,
    pub mesh_failure: u32,
    pub public_success: u32,
    pub public_failure: u32,
    pub fallback_success: u32,
    pub end_to_end_success: u32,
    pub end_to_end_failure: u32,
    pub latency_samples_ms: Vec<u32>,
    pub mesh_h2_requests: u32,
    pub mesh_connection_starts: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MeshPeerTelemetry {
    pub peer_id: String,
    pub peer_name: String,
    pub last_path: Option<TelemetryPath>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_route: Option<MeshActiveRoute>,
    pub last_sample_at: Option<String>,
    pub last_mesh_target: Option<String>,
    pub last_transition_at: Option<String>,
    pub breaker: Option<BreakerState>,
    pub last_mesh_reason: Option<MeshPeerReason>,
    pub last_mesh_protocol: Option<MeshTransportProtocol>,
    pub connection_generation: u64,
    pub current_connection_requests: u64,
    pub last_connection_started_at: Option<String>,
    pub buckets: VecDeque<MeshTelemetryBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedTelemetry {
    schema_version: u32,
    #[serde(default)]
    revision: u64,
    #[serde(default)]
    peers: BTreeMap<String, MeshPeerTelemetry>,
    #[serde(default)]
    events: VecDeque<MeshTelemetryEvent>,
}

struct TelemetryState {
    persisted: PersistedTelemetry,
    dirty: bool,
    retry_persist: bool,
    flush_scheduled: bool,
    last_sample_persist_at: Option<Instant>,
}

impl Default for PersistedTelemetry {
    fn default() -> Self {
        Self {
            schema_version: TELEMETRY_SCHEMA_VERSION,
            revision: 0,
            peers: BTreeMap::new(),
            events: VecDeque::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MeshTelemetrySnapshot {
    pub revision: u64,
    pub generated_at: String,
    pub peers: Vec<MeshPeerTelemetry>,
    pub events: Vec<MeshTelemetryEvent>,
}

#[derive(Debug, Clone, Copy)]
pub struct MeshTelemetrySample {
    pub path: TelemetryPath,
    pub success: bool,
    pub latency_ms: Option<u32>,
    pub fallback: bool,
    pub updates_active_path: bool,
    pub(crate) transport: Option<MeshTransportObservation>,
}

#[derive(Clone)]
pub struct MeshTelemetryHandle {
    path: Arc<PathBuf>,
    state: Arc<Mutex<TelemetryState>>,
    connections: MeshConnectionTrackers,
    probe_gate: Arc<Semaphore>,
    #[cfg(test)]
    persist_count: Arc<std::sync::atomic::AtomicUsize>,
}

impl MeshTelemetryHandle {
    pub fn load(data_dir: &Path) -> anyhow::Result<Self> {
        let path = data_dir.join("mesh").join("telemetry.json");
        let state = load_persisted_telemetry(&path, data_dir)?;
        Ok(Self {
            path: Arc::new(path),
            state: Arc::new(Mutex::new(TelemetryState {
                persisted: state,
                dirty: false,
                retry_persist: false,
                flush_scheduled: false,
                last_sample_persist_at: None,
            })),
            connections: MeshConnectionTrackers::default(),
            probe_gate: Arc::new(Semaphore::new(4)),
            #[cfg(test)]
            persist_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        })
    }

    /// Limits all scheduled and operator-triggered peer probes on this node together.
    pub fn probe_gate(&self) -> Arc<Semaphore> {
        self.probe_gate.clone()
    }

    pub async fn snapshot(&self) -> MeshTelemetrySnapshot {
        let state = self.state.lock().await;
        MeshTelemetrySnapshot {
            revision: state.persisted.revision,
            generated_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            peers: state.persisted.peers.values().cloned().collect(),
            events: state.persisted.events.iter().cloned().collect(),
        }
    }

    pub async fn record_sample(
        &self,
        peer_id: impl Into<String>,
        peer_name: impl Into<String>,
        sample: MeshTelemetrySample,
    ) -> anyhow::Result<()> {
        let now = Utc::now();
        let peer_id = peer_id.into();
        let observed_transport = self.connections.observe(&peer_id, sample.transport).await;
        let mut state = self.state.lock().await;
        let peer = state
            .persisted
            .peers
            .entry(peer_id.clone())
            .or_insert_with(|| MeshPeerTelemetry {
                peer_id: peer_id.clone(),
                ..MeshPeerTelemetry::default()
            });
        peer.peer_name = peer_name.into();
        let previous_path = peer.last_path;
        let updates_active_path = sample.updates_active_path && sample.success;
        if updates_active_path {
            peer.last_path = Some(sample.path);
            peer.active_route = Some(MeshActiveRoute {
                kind: match sample.path {
                    TelemetryPath::Mesh => ActiveRouteKind::RealityDirect,
                    TelemetryPath::Public => ActiveRouteKind::Public,
                },
                rendezvous: None,
                rendezvous_role: None,
                primary_rendezvous: None,
                standby_rendezvous: None,
                generation: None,
                readiness: None,
            });
        }
        peer.last_sample_at = Some(timestamp(now));
        if updates_active_path && previous_path != Some(sample.path) {
            peer.last_transition_at = Some(timestamp(now));
        }
        if let Some(observed) = observed_transport {
            peer.last_mesh_protocol = Some(observed.protocol);
            if observed.connection_started {
                peer.connection_generation = peer.connection_generation.saturating_add(1);
                peer.last_connection_started_at = Some(timestamp(now));
            }
            if let Some(requests) = observed.current_connection_requests {
                peer.current_connection_requests = requests;
            }
        }
        let bucket = ensure_bucket(peer, now);
        match (sample.path, sample.success) {
            (TelemetryPath::Mesh, true) => {
                bucket.mesh_success += 1;
                bucket.end_to_end_success += 1;
            }
            (TelemetryPath::Mesh, false) => bucket.mesh_failure += 1,
            (TelemetryPath::Public, true) => {
                bucket.public_success += 1;
                bucket.end_to_end_success += 1;
            }
            (TelemetryPath::Public, false) => {
                bucket.public_failure += 1;
                bucket.end_to_end_failure += 1;
            }
        }
        if sample.fallback && sample.success {
            bucket.fallback_success += 1;
        }
        if let Some(observed) = observed_transport {
            if observed.protocol == MeshTransportProtocol::H2 {
                bucket.mesh_h2_requests = bucket.mesh_h2_requests.saturating_add(1);
            }
            if observed.connection_started {
                bucket.mesh_connection_starts = bucket.mesh_connection_starts.saturating_add(1);
            }
        }
        if let Some(latency_ms) = sample.latency_ms {
            // Per-minute quantiles do not need unbounded precision. Keeping 64 values remains
            // stable under probe bursts while retaining enough signal for p50/p95.
            if bucket.latency_samples_ms.len() < 64 {
                bucket.latency_samples_ms.push(latency_ms);
            }
        }
        state.persisted.revision += 1;
        let deferred_flush = self.persist_sample_if_due(&mut state, Instant::now())?;
        drop(state);
        if let Some(delay) = deferred_flush {
            self.spawn_deferred_flush(delay);
        }
        Ok(())
    }

    pub async fn set_breaker(
        &self,
        peer_id: impl Into<String>,
        state_value: BreakerState,
        event_message: Option<String>,
    ) -> anyhow::Result<()> {
        let peer_id = peer_id.into();
        let now = Utc::now();
        let mut state = self.state.lock().await;
        let peer = state
            .persisted
            .peers
            .entry(peer_id.clone())
            .or_insert_with(|| MeshPeerTelemetry {
                peer_id: peer_id.clone(),
                ..MeshPeerTelemetry::default()
            });
        let previous = peer.breaker;
        peer.breaker = Some(state_value);
        if previous != Some(state_value) {
            if let Some(message) = event_message {
                push_event(
                    &mut state.persisted.events,
                    MeshTelemetryEvent {
                        at: timestamp(now),
                        peer_id,
                        kind: "breaker".to_string(),
                        message,
                    },
                );
            }
            state.persisted.revision += 1;
            self.persist_immediately(&mut state, Instant::now())?;
        }
        Ok(())
    }

    pub async fn set_mesh_reason(
        &self,
        peer_id: impl Into<String>,
        mesh_target: Option<&str>,
        reason: MeshPeerReason,
    ) -> anyhow::Result<()> {
        let peer_id = peer_id.into();
        let mut state = self.state.lock().await;
        let peer = state
            .persisted
            .peers
            .entry(peer_id.clone())
            .or_insert_with(|| MeshPeerTelemetry {
                peer_id,
                ..MeshPeerTelemetry::default()
            });
        let mesh_target = mesh_target.map(str::to_string);
        if peer.last_mesh_reason == Some(reason) && peer.last_mesh_target == mesh_target {
            return Ok(());
        }
        peer.last_mesh_reason = Some(reason);
        peer.last_mesh_target = mesh_target;
        state.persisted.revision += 1;
        self.persist_immediately(&mut state, Instant::now())
    }

    /// Records a final request failure after an earlier transport sample. This keeps path-attempt
    /// diagnostics separate from the end-to-end outcome so one failed Mesh attempt followed by a
    /// fallback remains one logical request.
    pub async fn record_terminal_failure(
        &self,
        peer_id: impl Into<String>,
        peer_name: impl Into<String>,
    ) -> anyhow::Result<()> {
        let now = Utc::now();
        let peer_id = peer_id.into();
        let mut state = self.state.lock().await;
        let peer = state
            .persisted
            .peers
            .entry(peer_id.clone())
            .or_insert_with(|| MeshPeerTelemetry {
                peer_id,
                ..MeshPeerTelemetry::default()
            });
        peer.peer_name = peer_name.into();
        peer.last_sample_at = Some(timestamp(now));
        ensure_bucket(peer, now).end_to_end_failure += 1;
        state.persisted.revision += 1;
        let deferred_flush = self.persist_sample_if_due(&mut state, Instant::now())?;
        drop(state);
        if let Some(delay) = deferred_flush {
            self.spawn_deferred_flush(delay);
        }
        Ok(())
    }

    pub async fn record_event(
        &self,
        peer_id: impl Into<String>,
        kind: impl Into<String>,
        message: impl Into<String>,
    ) -> anyhow::Result<()> {
        let mut state = self.state.lock().await;
        push_event(
            &mut state.persisted.events,
            MeshTelemetryEvent {
                at: timestamp(Utc::now()),
                peer_id: peer_id.into(),
                kind: kind.into(),
                message: message.into(),
            },
        );
        state.persisted.revision += 1;
        self.persist_immediately(&mut state, Instant::now())
    }

    fn persist_sample_if_due(
        &self,
        state: &mut TelemetryState,
        now: Instant,
    ) -> anyhow::Result<Option<StdDuration>> {
        state.dirty = true;
        let due = state.retry_persist
            || state.last_sample_persist_at.is_none_or(|last_persist| {
                now.duration_since(last_persist) >= SAMPLE_PERSIST_INTERVAL
            });
        if due {
            self.persist_locked(state, now)?;
            return Ok(None);
        }
        if state.flush_scheduled {
            return Ok(None);
        }
        state.flush_scheduled = true;
        let last_persist = state
            .last_sample_persist_at
            .expect("not-due telemetry persistence has a prior sample write");
        Ok(Some(
            SAMPLE_PERSIST_INTERVAL.saturating_sub(now.duration_since(last_persist)),
        ))
    }

    fn persist_immediately(&self, state: &mut TelemetryState, now: Instant) -> anyhow::Result<()> {
        state.dirty = true;
        self.persist_locked(state, now)
    }

    fn persist_locked(&self, state: &mut TelemetryState, now: Instant) -> anyhow::Result<()> {
        if let Err(error) = persist(&self.path, &state.persisted) {
            state.dirty = true;
            state.retry_persist = true;
            return Err(error);
        }
        state.dirty = false;
        state.retry_persist = false;
        state.last_sample_persist_at = Some(now);
        #[cfg(test)]
        self.persist_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    fn spawn_deferred_flush(&self, delay: StdDuration) {
        let telemetry = self.clone();
        tokio::spawn(async move {
            telemetry.flush_when_due(delay).await;
        });
    }

    async fn flush_when_due(&self, mut delay: StdDuration) {
        loop {
            tokio::time::sleep(delay).await;
            let mut state = self.state.lock().await;
            if !state.dirty {
                state.flush_scheduled = false;
                return;
            }
            let now = Instant::now();
            let Some(last_persist) = state.last_sample_persist_at else {
                state.flush_scheduled = false;
                if let Err(error) = self.persist_locked(&mut state, now) {
                    warn!(error = %error, "persist deferred mesh telemetry");
                }
                return;
            };
            let elapsed = now.duration_since(last_persist);
            if elapsed < SAMPLE_PERSIST_INTERVAL {
                delay = SAMPLE_PERSIST_INTERVAL - elapsed;
                drop(state);
                continue;
            }
            state.flush_scheduled = false;
            if let Err(error) = self.persist_locked(&mut state, now) {
                warn!(error = %error, "persist deferred mesh telemetry");
            }
            return;
        }
    }

    #[cfg(test)]
    fn persist_count(&self) -> usize {
        self.persist_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

pub fn quality_for_peer(peer: &MeshPeerTelemetry, now: DateTime<Utc>) -> MeshQuality {
    let Some(last_sample) = peer.last_sample_at.as_deref() else {
        return MeshQuality::Unknown;
    };
    let Ok(last_sample) = DateTime::parse_from_rfc3339(last_sample) else {
        return MeshQuality::Unknown;
    };
    if now.signed_duration_since(last_sample.with_timezone(&Utc)) > Duration::minutes(3) {
        return MeshQuality::Unknown;
    }
    let mut success = 0u32;
    let mut failure = 0u32;
    let mut latencies = Vec::new();
    for bucket in &peer.buckets {
        let (bucket_success, bucket_failure) = end_to_end_counts(bucket);
        success += bucket_success;
        failure += bucket_failure;
        latencies.extend_from_slice(&bucket.latency_samples_ms);
    }
    if success == 0 && failure > 0 {
        return MeshQuality::Down;
    }
    if failure > 0 {
        return MeshQuality::Unstable;
    }
    let p95 = percentile(&mut latencies, 0.95);
    match p95 {
        Some(value) if value >= 1000 => MeshQuality::Unstable,
        Some(value) if value >= 300 => MeshQuality::Slow,
        Some(_) => MeshQuality::Good,
        None => MeshQuality::Unknown,
    }
}

pub fn availability_for(peer: &MeshPeerTelemetry, minutes: i64, now: DateTime<Utc>) -> Option<f64> {
    let from = now - Duration::minutes(minutes);
    let (success, total) = peer
        .buckets
        .iter()
        .filter_map(|bucket| {
            DateTime::parse_from_rfc3339(&bucket.minute)
                .ok()
                .map(|at| (bucket, at))
        })
        .filter(|(_, at)| at.with_timezone(&Utc) >= from)
        .fold((0u32, 0u32), |(success, total), (bucket, _)| {
            let (bucket_success, bucket_failure) = end_to_end_counts(bucket);
            let bucket_total = bucket_success + bucket_failure;
            (success + bucket_success, total + bucket_total)
        });
    (total > 0).then_some(success as f64 / total as f64)
}

pub fn latency_percentiles_for(
    peer: &MeshPeerTelemetry,
    minutes: i64,
    now: DateTime<Utc>,
) -> (Option<u32>, Option<u32>) {
    let from = now - Duration::minutes(minutes);
    let mut values = peer
        .buckets
        .iter()
        .filter_map(|bucket| {
            DateTime::parse_from_rfc3339(&bucket.minute)
                .ok()
                .map(|at| (bucket, at))
        })
        .filter(|(_, at)| at.with_timezone(&Utc) >= from)
        .flat_map(|(bucket, _)| bucket.latency_samples_ms.iter().copied())
        .collect::<Vec<_>>();
    (
        percentile(&mut values.clone(), 0.5),
        percentile(&mut values, 0.95),
    )
}

fn ensure_bucket(peer: &mut MeshPeerTelemetry, now: DateTime<Utc>) -> &mut MeshTelemetryBucket {
    let minute = now
        .with_second(0)
        .and_then(|value| value.with_nanosecond(0))
        .unwrap_or(now);
    let minute_key = timestamp(minute);
    let cutoff = timestamp(now - Duration::hours(24));
    peer.buckets.retain(|bucket| bucket.minute >= cutoff);
    if !peer
        .buckets
        .iter()
        .any(|bucket| bucket.minute == minute_key)
    {
        peer.buckets.push_back(MeshTelemetryBucket {
            minute: minute_key,
            ..MeshTelemetryBucket::default()
        });
    }
    peer.buckets
        .make_contiguous()
        .sort_by(|left, right| left.minute.cmp(&right.minute));
    while peer.buckets.len() > MAX_BUCKETS {
        peer.buckets.pop_front();
    }
    let bucket_index = peer
        .buckets
        .iter()
        .position(|bucket| bucket.minute == timestamp(minute))
        .expect("bucket was inserted");
    peer.buckets
        .get_mut(bucket_index)
        .expect("bucket index is valid")
}

pub fn buckets_for_last_24_hours(
    peer: &MeshPeerTelemetry,
    now: DateTime<Utc>,
) -> Vec<MeshTelemetryBucket> {
    let now_minute = now
        .with_second(0)
        .and_then(|value| value.with_nanosecond(0))
        .unwrap_or(now);
    let indexed = peer
        .buckets
        .iter()
        .map(|bucket| (bucket.minute.as_str(), bucket))
        .collect::<BTreeMap<_, _>>();
    (0..MAX_BUCKETS)
        .map(|offset| {
            let minute = now_minute - Duration::minutes((MAX_BUCKETS - 1 - offset) as i64);
            let key = timestamp(minute);
            indexed
                .get(key.as_str())
                .cloned()
                .cloned()
                .unwrap_or(MeshTelemetryBucket {
                    minute: key,
                    ..MeshTelemetryBucket::default()
                })
        })
        .collect()
}

pub fn end_to_end_counts(bucket: &MeshTelemetryBucket) -> (u32, u32) {
    if bucket.end_to_end_success > 0 || bucket.end_to_end_failure > 0 {
        return (bucket.end_to_end_success, bucket.end_to_end_failure);
    }
    let success = bucket.mesh_success + bucket.public_success;
    let failure =
        (bucket.mesh_failure + bucket.public_failure).saturating_sub(bucket.fallback_success);
    (success, failure)
}

fn push_event(events: &mut VecDeque<MeshTelemetryEvent>, event: MeshTelemetryEvent) {
    events.push_back(event);
    while events.len() > MAX_EVENTS {
        events.pop_front();
    }
}

fn percentile(values: &mut [u32], fraction: f64) -> Option<u32> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let index = ((values.len() - 1) as f64 * fraction).ceil() as usize;
    values.get(index).copied()
}

fn timestamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn load_persisted_telemetry(path: &Path, data_dir: &Path) -> anyhow::Result<PersistedTelemetry> {
    match fs::read(path) {
        Ok(bytes) => parse_persisted_telemetry(&bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let Some(bytes) = read_legacy_mesh_telemetry(data_dir)
                .map_err(|error| anyhow::anyhow!(error.to_string()))?
            else {
                return Ok(PersistedTelemetry::default());
            };
            let persisted = parse_persisted_telemetry(&bytes)?;
            persist(path, &persisted)?;
            Ok(persisted)
        }
        Err(error) => Err(error.into()),
    }
}

fn parse_persisted_telemetry(bytes: &[u8]) -> anyhow::Result<PersistedTelemetry> {
    let parsed: PersistedTelemetry = serde_json::from_slice(bytes)?;
    if parsed.schema_version != TELEMETRY_SCHEMA_VERSION {
        anyhow::bail!(
            "mesh telemetry schema_version mismatch: expected {}, got {}",
            TELEMETRY_SCHEMA_VERSION,
            parsed.schema_version
        );
    }
    Ok(parsed)
}

fn persist(path: &Path, state: &PersistedTelemetry) -> anyhow::Result<()> {
    let parent = path.parent().expect("mesh telemetry path has a parent");
    fs::create_dir_all(parent)?;
    let bytes = serde_json::to_vec_pretty(state)?;
    let temporary = path.with_extension("json.tmp");
    {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

#[cfg(test)]
#[path = "mesh_telemetry/tests.rs"]
mod tests;
