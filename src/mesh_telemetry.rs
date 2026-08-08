use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::{DateTime, Duration, SecondsFormat, Timelike, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, Semaphore};

const TELEMETRY_SCHEMA_VERSION: u32 = 1;
const MAX_EVENTS: usize = 200;
const MAX_BUCKETS: usize = 24 * 60;

mod transport;
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
    state: Arc<Mutex<PersistedTelemetry>>,
    connections: MeshConnectionTrackers,
    probe_gate: Arc<Semaphore>,
}

impl MeshTelemetryHandle {
    pub fn load(data_dir: &Path) -> anyhow::Result<Self> {
        let path = data_dir.join("mesh").join("telemetry.json");
        let state = if path.exists() {
            let bytes = fs::read(&path)?;
            let parsed: PersistedTelemetry = serde_json::from_slice(&bytes)?;
            if parsed.schema_version != TELEMETRY_SCHEMA_VERSION {
                anyhow::bail!(
                    "mesh telemetry schema_version mismatch: expected {}, got {}",
                    TELEMETRY_SCHEMA_VERSION,
                    parsed.schema_version
                );
            }
            parsed
        } else {
            PersistedTelemetry::default()
        };
        Ok(Self {
            path: Arc::new(path),
            state: Arc::new(Mutex::new(state)),
            connections: MeshConnectionTrackers::default(),
            probe_gate: Arc::new(Semaphore::new(4)),
        })
    }

    /// Limits all scheduled and operator-triggered peer probes on this node together.
    pub fn probe_gate(&self) -> Arc<Semaphore> {
        self.probe_gate.clone()
    }

    pub async fn snapshot(&self) -> MeshTelemetrySnapshot {
        let state = self.state.lock().await;
        MeshTelemetrySnapshot {
            revision: state.revision,
            generated_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            peers: state.peers.values().cloned().collect(),
            events: state.events.iter().cloned().collect(),
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
        state.revision += 1;
        persist(&self.path, &state)
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
                    &mut state.events,
                    MeshTelemetryEvent {
                        at: timestamp(now),
                        peer_id,
                        kind: "breaker".to_string(),
                        message,
                    },
                );
            }
            state.revision += 1;
            persist(&self.path, &state)?;
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
        state.revision += 1;
        persist(&self.path, &state)
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
            .peers
            .entry(peer_id.clone())
            .or_insert_with(|| MeshPeerTelemetry {
                peer_id,
                ..MeshPeerTelemetry::default()
            });
        peer.peer_name = peer_name.into();
        peer.last_sample_at = Some(timestamp(now));
        ensure_bucket(peer, now).end_to_end_failure += 1;
        state.revision += 1;
        persist(&self.path, &state)
    }

    pub async fn record_event(
        &self,
        peer_id: impl Into<String>,
        kind: impl Into<String>,
        message: impl Into<String>,
    ) -> anyhow::Result<()> {
        let mut state = self.state.lock().await;
        push_event(
            &mut state.events,
            MeshTelemetryEvent {
                at: timestamp(Utc::now()),
                peer_id: peer_id.into(),
                kind: kind.into(),
                message: message.into(),
            },
        );
        state.revision += 1;
        persist(&self.path, &state)
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

fn persist(path: &Path, state: &PersistedTelemetry) -> anyhow::Result<()> {
    let parent = path.parent().expect("mesh telemetry path has parent");
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
mod tests {
    use super::*;
    use std::net::SocketAddr;

    fn h2_sample(fingerprint: MeshConnectionFingerprint) -> MeshTelemetrySample {
        MeshTelemetrySample {
            path: TelemetryPath::Mesh,
            success: true,
            latency_ms: Some(10),
            fallback: false,
            updates_active_path: true,
            transport: Some(MeshTransportObservation {
                protocol: MeshTransportProtocol::H2,
                fingerprint: Some(fingerprint),
            }),
        }
    }

    fn fingerprint(local_port: u16, remote_port: u16) -> MeshConnectionFingerprint {
        MeshConnectionFingerprint {
            local_addr: SocketAddr::from(([127, 0, 0, 1], local_port)),
            remote_addr: SocketAddr::from(([127, 0, 0, 2], remote_port)),
        }
    }

    #[tokio::test]
    async fn persists_bounded_buckets_and_events() {
        let temp = tempfile::tempdir().unwrap();
        let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
        telemetry
            .record_sample(
                "peer-a",
                "alpha",
                MeshTelemetrySample {
                    path: TelemetryPath::Mesh,
                    success: true,
                    latency_ms: Some(xp_test_fixtures::slot_n27()),
                    fallback: false,
                    updates_active_path: true,
                    transport: None,
                },
            )
            .await
            .unwrap();
        telemetry
            .set_breaker(
                "peer-a",
                BreakerState::Open,
                Some("three mesh failures".to_string()),
            )
            .await
            .unwrap();
        let restored = MeshTelemetryHandle::load(temp.path()).unwrap();
        let snapshot = restored.snapshot().await;
        assert_eq!(snapshot.peers.len(), 1);
        assert_eq!(snapshot.peers[0].buckets[0].mesh_success, 1);
        assert_eq!(snapshot.events.len(), 1);
    }

    #[tokio::test]
    async fn connection_fingerprints_count_reuse_without_persisting_socket_addresses() {
        let temp = tempfile::tempdir().unwrap();
        let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
        telemetry
            .record_sample("peer-a", "alpha", h2_sample(fingerprint(41000, 443)))
            .await
            .unwrap();
        telemetry
            .record_sample("peer-a", "alpha", h2_sample(fingerprint(41000, 443)))
            .await
            .unwrap();
        telemetry
            .record_sample("peer-a", "alpha", h2_sample(fingerprint(41001, 443)))
            .await
            .unwrap();

        let peer = telemetry.snapshot().await.peers.remove(0);
        assert_eq!(peer.last_mesh_protocol, Some(MeshTransportProtocol::H2));
        assert_eq!(peer.connection_generation, 2);
        assert_eq!(peer.current_connection_requests, 1);
        assert_eq!(peer.buckets[0].mesh_h2_requests, 3);
        assert_eq!(peer.buckets[0].mesh_connection_starts, 2);

        let persisted = fs::read_to_string(temp.path().join("mesh/telemetry.json")).unwrap();
        assert!(!persisted.contains("127.0.0.1"));
        assert!(!persisted.contains("41000"));
        assert!(!persisted.contains("41001"));
    }

    #[tokio::test]
    async fn first_connection_after_restart_advances_the_persisted_generation() {
        let temp = tempfile::tempdir().unwrap();
        let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
        telemetry
            .record_sample("peer-a", "alpha", h2_sample(fingerprint(41000, 443)))
            .await
            .unwrap();
        drop(telemetry);

        let restored = MeshTelemetryHandle::load(temp.path()).unwrap();
        restored
            .record_sample("peer-a", "alpha", h2_sample(fingerprint(41000, 443)))
            .await
            .unwrap();
        let peer = restored.snapshot().await.peers.remove(0);

        assert_eq!(peer.connection_generation, 2);
        assert_eq!(peer.current_connection_requests, 1);
        assert_eq!(peer.buckets[0].mesh_connection_starts, 2);
    }

    #[tokio::test]
    async fn persists_mesh_reason_and_reads_legacy_peer_records() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("mesh/telemetry.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{
                "schema_version": 1,
                "revision": 0,
                "peers": {"peer-a": {"peer_id": "peer-a", "buckets": []}},
                "events": []
            }"#,
        )
        .unwrap();
        let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
        let legacy_snapshot = telemetry.snapshot().await;
        let legacy_peer = &legacy_snapshot.peers[0];
        assert_eq!(legacy_peer.last_mesh_reason, None);
        assert_eq!(legacy_peer.last_mesh_protocol, None);
        assert_eq!(legacy_peer.connection_generation, 0);
        telemetry
            .set_mesh_reason(
                "peer-a",
                Some("https://peer-a.example.test:443"),
                MeshPeerReason::TransportTimeout,
            )
            .await
            .unwrap();
        let restored = MeshTelemetryHandle::load(temp.path()).unwrap();
        assert_eq!(
            restored.snapshot().await.peers[0].last_mesh_reason,
            Some(MeshPeerReason::TransportTimeout)
        );
    }

    #[tokio::test]
    async fn probe_gate_is_shared_by_telemetry_clones() {
        let temp = tempfile::tempdir().unwrap();
        let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
        let shared_gate = telemetry.clone().probe_gate();
        let gate = telemetry.probe_gate();
        let permits = [
            gate.clone().acquire_owned().await.unwrap(),
            gate.clone().acquire_owned().await.unwrap(),
            gate.clone().acquire_owned().await.unwrap(),
            gate.clone().acquire_owned().await.unwrap(),
        ];

        assert!(shared_gate.try_acquire().is_err());
        drop(permits);
        assert!(shared_gate.try_acquire().is_ok());
    }

    #[test]
    fn quality_uses_end_to_end_result_and_latency() {
        let now = xp_test_fixtures::baseline_timestamp()
            .parse::<DateTime<Utc>>()
            .unwrap();
        let peer = MeshPeerTelemetry {
            last_sample_at: Some(xp_test_fixtures::baseline_timestamp().to_owned()),
            buckets: VecDeque::from([MeshTelemetryBucket {
                minute: xp_test_fixtures::baseline_timestamp().to_owned(),
                public_success: 1,
                fallback_success: 1,
                end_to_end_success: 1,
                latency_samples_ms: vec![120],
                ..MeshTelemetryBucket::default()
            }]),
            ..MeshPeerTelemetry::default()
        };
        assert_eq!(quality_for_peer(&peer, now), MeshQuality::Good);
    }

    #[test]
    fn fallback_is_one_successful_logical_request() {
        let now = xp_test_fixtures::baseline_timestamp()
            .parse::<DateTime<Utc>>()
            .unwrap();
        let peer = MeshPeerTelemetry {
            last_sample_at: Some(xp_test_fixtures::baseline_timestamp().to_owned()),
            buckets: VecDeque::from([MeshTelemetryBucket {
                minute: xp_test_fixtures::baseline_timestamp().to_owned(),
                mesh_failure: 1,
                public_success: 1,
                fallback_success: 1,
                end_to_end_success: 1,
                latency_samples_ms: vec![120],
                ..MeshTelemetryBucket::default()
            }]),
            ..MeshPeerTelemetry::default()
        };
        assert_eq!(availability_for(&peer, 60, now), Some(1.0));
        assert_eq!(quality_for_peer(&peer, now), MeshQuality::Good);
    }

    #[test]
    fn mesh_transport_health_uses_the_five_minute_churn_threshold() {
        let now = Utc::now();
        assert_eq!(
            mesh_transport_health_for(None, now),
            MeshTransportHealth::Unknown
        );
        let mut peer = MeshPeerTelemetry {
            last_mesh_protocol: Some(MeshTransportProtocol::H2),
            connection_generation: 3,
            buckets: VecDeque::from([
                MeshTelemetryBucket {
                    minute: timestamp(now - Duration::minutes(6)),
                    mesh_h2_requests: 8,
                    mesh_connection_starts: 8,
                    ..MeshTelemetryBucket::default()
                },
                MeshTelemetryBucket {
                    minute: timestamp(now - Duration::minutes(2)),
                    mesh_h2_requests: 12,
                    mesh_connection_starts: 2,
                    ..MeshTelemetryBucket::default()
                },
            ]),
            ..MeshPeerTelemetry::default()
        };
        assert_eq!(
            mesh_transport_health_for(Some(&peer), now),
            MeshTransportHealth::Healthy
        );
        peer.buckets.back_mut().unwrap().mesh_connection_starts = 3;
        assert_eq!(
            mesh_transport_health_for(Some(&peer), now),
            MeshTransportHealth::Churning
        );
        peer.last_mesh_protocol = Some(MeshTransportProtocol::Other);
        assert_eq!(
            mesh_transport_health_for(Some(&peer), now),
            MeshTransportHealth::Churning
        );
    }

    #[test]
    fn mesh_transport_counts_are_bounded_to_the_requested_window() {
        let now = Utc::now();
        let peer = MeshPeerTelemetry {
            buckets: VecDeque::from([
                MeshTelemetryBucket {
                    minute: timestamp(now - Duration::minutes(61)),
                    mesh_h2_requests: 100,
                    mesh_connection_starts: 100,
                    ..MeshTelemetryBucket::default()
                },
                MeshTelemetryBucket {
                    minute: timestamp(now - Duration::minutes(30)),
                    mesh_h2_requests: 20,
                    mesh_connection_starts: 2,
                    ..MeshTelemetryBucket::default()
                },
                MeshTelemetryBucket {
                    minute: timestamp(now - Duration::minutes(2)),
                    mesh_h2_requests: 5,
                    mesh_connection_starts: 1,
                    ..MeshTelemetryBucket::default()
                },
            ]),
            ..MeshPeerTelemetry::default()
        };

        assert_eq!(mesh_transport_counts_for(&peer, 5, now), (5, 1));
        assert_eq!(mesh_transport_counts_for(&peer, 60, now), (25, 3));
    }

    #[tokio::test]
    async fn passive_public_sample_does_not_replace_active_mesh_path() {
        let temp = tempfile::tempdir().unwrap();
        let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
        telemetry
            .record_sample(
                "peer-a",
                "alpha",
                MeshTelemetrySample {
                    path: TelemetryPath::Mesh,
                    success: true,
                    latency_ms: Some(xp_test_fixtures::slot_n27()),
                    fallback: false,
                    updates_active_path: true,
                    transport: None,
                },
            )
            .await
            .unwrap();
        let before = telemetry.snapshot().await.peers.remove(0);

        telemetry
            .record_sample(
                "peer-a",
                "alpha",
                MeshTelemetrySample {
                    path: TelemetryPath::Public,
                    success: true,
                    latency_ms: Some(xp_test_fixtures::slot_n28()),
                    fallback: false,
                    updates_active_path: false,
                    transport: None,
                },
            )
            .await
            .unwrap();
        let after = telemetry.snapshot().await.peers.remove(0);
        assert_eq!(after.last_path, Some(TelemetryPath::Mesh));
        assert_eq!(after.last_transition_at, before.last_transition_at);
        assert_eq!(after.buckets[0].public_success, 1);
    }

    #[tokio::test]
    async fn mesh_attempt_failure_does_not_flap_an_already_active_public_path() {
        let temp = tempfile::tempdir().unwrap();
        let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
        telemetry
            .record_sample(
                "peer-a",
                "alpha",
                MeshTelemetrySample {
                    path: TelemetryPath::Public,
                    success: true,
                    latency_ms: Some(xp_test_fixtures::slot_n28()),
                    fallback: false,
                    updates_active_path: true,
                    transport: None,
                },
            )
            .await
            .unwrap();
        {
            let mut state = telemetry.state.lock().await;
            state.peers.get_mut("peer-a").unwrap().last_transition_at =
                Some(xp_test_fixtures::baseline_timestamp().to_owned());
        }

        telemetry
            .record_sample(
                "peer-a",
                "alpha",
                MeshTelemetrySample {
                    path: TelemetryPath::Mesh,
                    success: false,
                    latency_ms: xp_test_fixtures::none(),
                    fallback: false,
                    updates_active_path: true,
                    transport: None,
                },
            )
            .await
            .unwrap();
        telemetry
            .record_sample(
                "peer-a",
                "alpha",
                MeshTelemetrySample {
                    path: TelemetryPath::Public,
                    success: true,
                    latency_ms: Some(xp_test_fixtures::slot_n29()),
                    fallback: true,
                    updates_active_path: true,
                    transport: None,
                },
            )
            .await
            .unwrap();

        let peer = telemetry.snapshot().await.peers.remove(0);
        assert_eq!(peer.last_path, Some(TelemetryPath::Public));
        assert_eq!(
            peer.last_transition_at.as_deref(),
            Some(xp_test_fixtures::baseline_timestamp())
        );
    }

    #[tokio::test]
    async fn terminal_mesh_failure_contributes_one_end_to_end_failure() {
        let temp = tempfile::tempdir().unwrap();
        let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
        telemetry
            .record_sample(
                "peer-a",
                "alpha",
                MeshTelemetrySample {
                    path: TelemetryPath::Mesh,
                    success: false,
                    latency_ms: xp_test_fixtures::none(),
                    fallback: false,
                    updates_active_path: false,
                    transport: None,
                },
            )
            .await
            .unwrap();
        telemetry
            .record_terminal_failure("peer-a", "alpha")
            .await
            .unwrap();

        let peer = telemetry.snapshot().await.peers.remove(0);
        assert_eq!(end_to_end_counts(&peer.buckets[0]), (0, 1));
    }

    #[test]
    fn buckets_are_pruned_by_age_and_expand_unknown_minutes() {
        let now = xp_test_fixtures::baseline_timestamp()
            .parse::<DateTime<Utc>>()
            .unwrap();
        let mut peer = MeshPeerTelemetry {
            buckets: VecDeque::from([MeshTelemetryBucket {
                minute: xp_test_fixtures::slot_s623().to_owned(),
                mesh_success: 1,
                ..MeshTelemetryBucket::default()
            }]),
            ..MeshPeerTelemetry::default()
        };
        let bucket = ensure_bucket(&mut peer, now);
        bucket.mesh_success = 1;
        assert_eq!(peer.buckets.len(), 1);
        let timeline = buckets_for_last_24_hours(&peer, now);
        assert_eq!(timeline.len(), MAX_BUCKETS);
        assert_eq!(timeline[0].mesh_success, 0);
        assert_eq!(timeline[MAX_BUCKETS - 1].mesh_success, 1);
    }
}
