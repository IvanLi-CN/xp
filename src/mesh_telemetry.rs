use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::{DateTime, Duration, SecondsFormat, Timelike, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

const TELEMETRY_SCHEMA_VERSION: u32 = 1;
const MAX_EVENTS: usize = 200;
const MAX_BUCKETS: usize = 24 * 60;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshTelemetryEvent {
    pub at: String,
    pub peer_id: String,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MeshTelemetryBucket {
    pub minute: String,
    #[serde(default)]
    pub mesh_success: u32,
    #[serde(default)]
    pub mesh_failure: u32,
    #[serde(default)]
    pub public_success: u32,
    #[serde(default)]
    pub public_failure: u32,
    #[serde(default)]
    pub fallback_success: u32,
    #[serde(default)]
    pub end_to_end_success: u32,
    #[serde(default)]
    pub end_to_end_failure: u32,
    #[serde(default)]
    pub latency_samples_ms: Vec<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MeshPeerTelemetry {
    pub peer_id: String,
    #[serde(default)]
    pub peer_name: String,
    #[serde(default)]
    pub last_path: Option<TelemetryPath>,
    #[serde(default)]
    pub last_sample_at: Option<String>,
    #[serde(default)]
    pub last_transition_at: Option<String>,
    #[serde(default)]
    pub breaker: Option<BreakerState>,
    #[serde(default)]
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

#[derive(Clone)]
pub struct MeshTelemetryHandle {
    path: Arc<PathBuf>,
    state: Arc<Mutex<PersistedTelemetry>>,
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
        })
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
        path: TelemetryPath,
        success: bool,
        latency_ms: Option<u32>,
        fallback: bool,
    ) -> anyhow::Result<()> {
        let now = Utc::now();
        let peer_id = peer_id.into();
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
        peer.last_path = Some(path);
        peer.last_sample_at = Some(timestamp(now));
        if previous_path != Some(path) {
            peer.last_transition_at = Some(timestamp(now));
        }
        let bucket = ensure_bucket(peer, now);
        match (path, success) {
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
        if fallback && success {
            bucket.fallback_success += 1;
        }
        if let Some(latency_ms) = latency_ms {
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
            peer.last_transition_at = Some(timestamp(now));
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

    #[tokio::test]
    async fn persists_bounded_buckets_and_events() {
        let temp = tempfile::tempdir().unwrap();
        let telemetry = MeshTelemetryHandle::load(temp.path()).unwrap();
        telemetry
            .record_sample(
                "peer-a",
                "alpha",
                TelemetryPath::Mesh,
                true,
                Some(42),
                false,
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

    #[test]
    fn quality_uses_end_to_end_result_and_latency() {
        let now = Utc::now();
        let peer = MeshPeerTelemetry {
            last_sample_at: Some(timestamp(now)),
            buckets: VecDeque::from([MeshTelemetryBucket {
                minute: timestamp(now),
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
        let now = Utc::now();
        let peer = MeshPeerTelemetry {
            last_sample_at: Some(timestamp(now)),
            buckets: VecDeque::from([MeshTelemetryBucket {
                minute: timestamp(now),
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
    fn buckets_are_pruned_by_age_and_expand_unknown_minutes() {
        let now = Utc::now();
        let mut peer = MeshPeerTelemetry {
            buckets: VecDeque::from([MeshTelemetryBucket {
                minute: timestamp(now - Duration::hours(25)),
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
