use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use chrono::{DateTime, Days, Duration as ChronoDuration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{Mutex, RwLock},
    time::MissedTickBehavior,
};
use tracing::warn;

use crate::{
    config::Config,
    control_plane_mesh::{MeshAwareHttpClient, peer_target_from_node},
    cycle::{CycleTimeZone, current_cycle_window_at},
    domain::{Node, NodeQuotaReset, User, UserQuotaReset},
    node_runtime::{
        LocalNodeRuntimeSnapshot, NodeRuntimeEventKind, NodeRuntimeHandle, RuntimeComponent,
        RuntimeStatus,
    },
    state::{JsonSnapshotStore, membership_xray_email},
    xray,
};

#[path = "node_history_remote.rs"]
mod remote;

const HISTORY_SCHEMA_VERSION: u32 = 2;
const HISTORY_WINDOW_DAYS: u64 = 90;
const TRAFFIC_ROLLUP_WINDOW_SECS: i64 = 49 * 60 * 60;
const TRAFFIC_ROLLUP_BUCKET_SECS: i64 = 5 * 60;
const TRAFFIC_ROLLUP_BUCKETS: usize = 588;
const TRAFFIC_DAILY_BUCKETS: usize = 90;
const EVENT_WINDOW_DAYS: u64 = 7;
const MAX_EVENTS_PER_NODE: usize = 50;
const SYNC_INTERVAL_SECS: u64 = 5 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeHistoryDailyTraffic {
    pub date: String,
    pub uplink_bytes: u64,
    pub downlink_bytes: u64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeHistoryDailyComponentStatus {
    pub date: String,
    pub components: Vec<NodeHistoryComponentDayStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeHistoryComponentDayStatus {
    pub component: RuntimeComponent,
    pub status: RuntimeStatus,
    pub observed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeHistoryComponentStatusEvent {
    pub event_id: String,
    pub occurred_at: String,
    pub component: RuntimeComponent,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_status: Option<RuntimeStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_status: Option<RuntimeStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeHistorySnapshot {
    pub node_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_synced_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sync_error: Option<String>,
    pub daily_traffic: Vec<NodeHistoryDailyTraffic>,
    pub daily_component_status: Vec<NodeHistoryDailyComponentStatus>,
    pub component_status_events: Vec<NodeHistoryComponentStatusEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traffic: Option<NodeTrafficRollupSnapshot>,
    /// Bounded user ID index mirrored with node history for post-membership queries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user_traffic_users: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeTrafficBucket {
    pub start_at: String,
    pub end_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uplink_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downlink_bytes: Option<u64>,
    pub complete: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeTrafficDailyBucket {
    pub date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uplink_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downlink_bytes: Option<u64>,
    pub complete: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrafficCycleAccumulator {
    #[serde(default = "default_monthly_cycle_mode")]
    pub mode: String,
    pub start_at: String,
    pub end_at: String,
    pub uplink_bytes: u64,
    pub downlink_bytes: u64,
    pub complete: bool,
    pub tracking_since: String,
    #[serde(default)]
    pub warnings: Vec<String>,
}

fn default_monthly_cycle_mode() -> String {
    "monthly".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct NodeTrafficRollupSnapshot {
    #[serde(default)]
    pub five_minute: Vec<NodeTrafficBucket>,
    #[serde(default)]
    pub daily: Vec<NodeTrafficDailyBucket>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycle: Option<TrafficCycleAccumulator>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sample_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrafficSeriesPoint {
    pub start_at: String,
    pub end_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uplink_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downlink_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    pub complete: bool,
    pub is_current_day: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrafficSummary {
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycle_start_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycle_end_at: Option<String>,
    pub uplink_bytes: u64,
    pub downlink_bytes: u64,
    pub total_bytes: u64,
    pub complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracking_since: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrafficReport {
    pub window: String,
    pub window_start_at: String,
    pub window_end_at: String,
    pub timezone: String,
    pub summary: TrafficSummary,
    pub current: Vec<TrafficSeriesPoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<Vec<TrafficSeriesPoint>>,
    pub partial: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sample_at: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserTrafficNodeOption {
    pub node_id: String,
    pub node_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserTrafficReport {
    pub report: TrafficReport,
    pub nodes: Vec<UserTrafficNodeOption>,
    pub partial: bool,
    #[serde(default)]
    pub unreachable_nodes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TrafficCycleContext {
    pub start_at: String,
    pub end_at: String,
    pub mode: TrafficCycleMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrafficCycleMode {
    Monthly,
    Unlimited,
}

#[derive(Debug, Clone)]
pub struct NodeTrafficSample {
    pub totals: Vec<NodeTrafficTotals>,
    pub unavailable_users: BTreeSet<String>,
    pub complete: bool,
    pub warnings: Vec<String>,
    pub cycle: Option<TrafficCycleContext>,
    pub user_cycles: BTreeMap<String, TrafficCycleContext>,
}

struct UserTrafficDelta {
    uplink: u64,
    downlink: u64,
    known: bool,
    complete: bool,
    warnings: Vec<String>,
    gap_dates: BTreeSet<String>,
}

impl Default for UserTrafficDelta {
    fn default() -> Self {
        Self {
            uplink: 0,
            downlink: 0,
            known: false,
            complete: true,
            warnings: Vec::new(),
            gap_dates: BTreeSet::new(),
        }
    }
}

fn default_membership_active() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct TrafficBaseline {
    uplink_total: u64,
    downlink_total: u64,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PersistedNodeHistoryRecord {
    node_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_synced_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_sync_error: Option<String>,
    #[serde(default)]
    daily_traffic: BTreeMap<String, NodeHistoryDailyTraffic>,
    #[serde(default)]
    daily_component_status: BTreeMap<String, NodeHistoryDailyComponentStatus>,
    #[serde(default)]
    component_status_events: Vec<NodeHistoryComponentStatusEvent>,
    #[serde(default)]
    traffic_baselines: BTreeMap<String, TrafficBaseline>,
    #[serde(default)]
    traffic_rollup: NodeTrafficRollupSnapshot,
    #[serde(default)]
    user_traffic: BTreeMap<String, PersistedUserTrafficRecord>,
    #[serde(default)]
    user_traffic_users: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
struct PersistedUserTrafficRecord {
    #[serde(default)]
    five_minute: Vec<NodeTrafficBucket>,
    #[serde(default)]
    daily: Vec<NodeTrafficDailyBucket>,
    #[serde(default)]
    baselines: BTreeMap<String, TrafficBaseline>,
    #[serde(default)]
    cycle: Option<TrafficCycleAccumulator>,
    #[serde(default)]
    last_sample_at: Option<String>,
    #[serde(default = "default_membership_active")]
    membership_active: bool,
}

impl PersistedNodeHistoryRecord {
    fn empty(node_id: String) -> Self {
        Self {
            node_id,
            last_synced_at: None,
            last_sync_error: None,
            daily_traffic: BTreeMap::new(),
            daily_component_status: BTreeMap::new(),
            component_status_events: Vec::new(),
            traffic_baselines: BTreeMap::new(),
            traffic_rollup: NodeTrafficRollupSnapshot::default(),
            user_traffic: BTreeMap::new(),
            user_traffic_users: BTreeSet::new(),
        }
    }

    fn snapshot(&self) -> NodeHistorySnapshot {
        let mut user_traffic_users = self.user_traffic_users.clone();
        user_traffic_users.extend(self.user_traffic.keys().cloned());
        NodeHistorySnapshot {
            node_id: self.node_id.clone(),
            last_synced_at: self.last_synced_at.clone(),
            last_sync_error: self.last_sync_error.clone(),
            daily_traffic: self.daily_traffic.values().cloned().collect(),
            daily_component_status: self.daily_component_status.values().cloned().collect(),
            component_status_events: self.component_status_events.clone(),
            traffic: Some(self.traffic_rollup.clone()),
            user_traffic_users: user_traffic_users.into_iter().collect(),
        }
    }

    fn prune(&mut self, now: DateTime<Utc>) {
        let cutoff = date_key(now - Days::new(HISTORY_WINDOW_DAYS));
        self.daily_traffic.retain(|date, _| date >= &cutoff);
        self.daily_component_status
            .retain(|date, _| date >= &cutoff);

        let event_cutoff = rfc3339(now - Days::new(EVENT_WINDOW_DAYS));
        self.component_status_events
            .retain(|event| event.occurred_at >= event_cutoff);
        self.component_status_events
            .sort_by(|a, b| b.occurred_at.cmp(&a.occurred_at));
        self.component_status_events.truncate(MAX_EVENTS_PER_NODE);

        let traffic_cutoff = rfc3339(now - ChronoDuration::seconds(TRAFFIC_ROLLUP_WINDOW_SECS));
        self.traffic_rollup
            .five_minute
            .retain(|bucket| bucket.end_at > traffic_cutoff);
        self.traffic_rollup
            .five_minute
            .sort_by(|a, b| a.start_at.cmp(&b.start_at));
        if self.traffic_rollup.five_minute.len() > TRAFFIC_ROLLUP_BUCKETS {
            let drop_count = self.traffic_rollup.five_minute.len() - TRAFFIC_ROLLUP_BUCKETS;
            self.traffic_rollup.five_minute.drain(0..drop_count);
        }
        self.traffic_rollup
            .daily
            .retain(|bucket| bucket.date >= date_key(now - Days::new(89)));
        self.traffic_rollup
            .daily
            .sort_by(|a, b| a.date.cmp(&b.date));
        if self.traffic_rollup.daily.len() > TRAFFIC_DAILY_BUCKETS {
            let drop_count = self.traffic_rollup.daily.len() - TRAFFIC_DAILY_BUCKETS;
            self.traffic_rollup.daily.drain(0..drop_count);
        }
        for user in self.user_traffic.values_mut() {
            user.five_minute
                .retain(|bucket| bucket.end_at > traffic_cutoff);
            user.five_minute.sort_by(|a, b| a.start_at.cmp(&b.start_at));
            if user.five_minute.len() > TRAFFIC_ROLLUP_BUCKETS {
                let drop_count = user.five_minute.len() - TRAFFIC_ROLLUP_BUCKETS;
                user.five_minute.drain(0..drop_count);
            }
            user.daily
                .retain(|bucket| bucket.date >= date_key(now - Days::new(89)));
            user.daily.sort_by(|a, b| a.date.cmp(&b.date));
            if user.daily.len() > TRAFFIC_DAILY_BUCKETS {
                let drop_count = user.daily.len() - TRAFFIC_DAILY_BUCKETS;
                user.daily.drain(0..drop_count);
            }
            user.baselines
                .retain(|_, baseline| baseline.updated_at > traffic_cutoff);
        }
        self.user_traffic.retain(|_, user| {
            user.membership_active || !user.five_minute.is_empty() || !user.daily.is_empty()
        });
        self.traffic_baselines
            .retain(|_, baseline| baseline.updated_at > traffic_cutoff);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedNodeHistoryCache {
    schema_version: u32,
    #[serde(default)]
    nodes: BTreeMap<String, PersistedNodeHistoryRecord>,
    #[serde(default)]
    pending_user_traffic_cleanup: BTreeMap<String, BTreeSet<String>>,
    #[serde(default)]
    pending_node_history_cleanup: BTreeMap<String, BTreeSet<String>>,
    #[serde(default)]
    deleted_users: BTreeSet<String>,
    #[serde(default)]
    deleted_nodes: BTreeSet<String>,
}

impl PersistedNodeHistoryCache {
    fn empty() -> Self {
        Self {
            schema_version: HISTORY_SCHEMA_VERSION,
            nodes: BTreeMap::new(),
            pending_user_traffic_cleanup: BTreeMap::new(),
            pending_node_history_cleanup: BTreeMap::new(),
            deleted_users: BTreeSet::new(),
            deleted_nodes: BTreeSet::new(),
        }
    }
}

#[derive(Clone)]
pub struct NodeHistoryHandle {
    inner: Arc<RwLock<PersistedNodeHistoryCache>>,
    persistence_path: Arc<PathBuf>,
    persistence_lock: Arc<Mutex<()>>,
}

impl NodeHistoryHandle {
    pub fn from_config(config: &Config) -> Self {
        Self::new(config.data_dir.join("node_history_cache.json"))
    }

    fn new(persistence_path: PathBuf) -> Self {
        let cache =
            load_history_cache(&persistence_path).unwrap_or_else(PersistedNodeHistoryCache::empty);
        Self {
            inner: Arc::new(RwLock::new(cache)),
            persistence_path: Arc::new(persistence_path),
            persistence_lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn snapshot(&self, node_id: &str) -> Option<NodeHistorySnapshot> {
        let state = self.inner.read().await;
        state
            .nodes
            .get(node_id)
            .map(PersistedNodeHistoryRecord::snapshot)
    }

    pub async fn record_local_sample(
        &self,
        now: DateTime<Utc>,
        node_id: &str,
        traffic_totals: Option<Vec<NodeTrafficTotals>>,
        runtime: LocalNodeRuntimeSnapshot,
    ) {
        let sample = traffic_totals.map(|totals| NodeTrafficSample {
            totals,
            unavailable_users: BTreeSet::new(),
            complete: true,
            warnings: Vec::new(),
            cycle: None,
            user_cycles: BTreeMap::new(),
        });
        self.record_local_sample_with_status(now, node_id, sample, runtime)
            .await;
    }

    pub async fn record_local_sample_with_status(
        &self,
        now: DateTime<Utc>,
        node_id: &str,
        sample: Option<NodeTrafficSample>,
        runtime: LocalNodeRuntimeSnapshot,
    ) {
        {
            let mut state = self.inner.write().await;
            if state.deleted_nodes.contains(node_id) {
                return;
            }
            let deleted_users = state.deleted_users.clone();
            let record = state
                .nodes
                .entry(node_id.to_string())
                .or_insert_with(|| PersistedNodeHistoryRecord::empty(node_id.to_string()));
            record.last_synced_at = Some(rfc3339(now));
            record.last_sync_error = None;

            if let Some(sample) = sample {
                record_traffic_sample(record, now, sample, &deleted_users);
            }
            record_daily_components(record, now, runtime);
            record.prune(now);
        }
        self.persist().await;
    }

    pub async fn replace_node_snapshot(
        &self,
        now: DateTime<Utc>,
        node_id: &str,
        snapshot: NodeHistorySnapshot,
    ) {
        {
            let mut state = self.inner.write().await;
            if state.deleted_nodes.contains(node_id) {
                return;
            }
            let mut record = state
                .nodes
                .remove(node_id)
                .unwrap_or_else(|| PersistedNodeHistoryRecord::empty(node_id.to_string()));
            record.node_id = node_id.to_string();
            record.last_synced_at = Some(rfc3339(now));
            record.last_sync_error = None;
            record.traffic_baselines.clear();
            record.daily_traffic = snapshot
                .daily_traffic
                .into_iter()
                .map(|item| (item.date.clone(), item))
                .collect();
            record.daily_component_status = snapshot
                .daily_component_status
                .into_iter()
                .map(|item| (item.date.clone(), item))
                .collect();
            record.component_status_events = snapshot.component_status_events;
            record.user_traffic.clear();
            record.user_traffic_users = snapshot.user_traffic_users.into_iter().collect();
            if let Some(traffic) = snapshot.traffic {
                record.traffic_rollup = traffic;
            }
            record.prune(now);
            state.nodes.insert(node_id.to_string(), record);
        }
        self.persist().await;
    }

    pub async fn clear_node(&self, node_id: &str) {
        {
            let mut state = self.inner.write().await;
            state.deleted_nodes.insert(node_id.to_string());
            state.nodes.remove(node_id);
            state.pending_user_traffic_cleanup.remove(node_id);
            state.pending_node_history_cleanup.remove(node_id);
            for targets in state.pending_node_history_cleanup.values_mut() {
                targets.remove(node_id);
            }
            state
                .pending_node_history_cleanup
                .retain(|_, targets| !targets.is_empty());
        }
        self.persist().await;
    }

    pub async fn clear_user(&self, user_id: &str) {
        {
            let mut state = self.inner.write().await;
            state.deleted_users.insert(user_id.to_string());
            for record in state.nodes.values_mut() {
                record.user_traffic.remove(user_id);
                record.user_traffic_users.remove(user_id);
            }
        }
        self.persist().await;
    }

    pub async fn queue_user_traffic_cleanup(&self, node_id: &str, user_id: &str) {
        let changed = {
            let mut state = self.inner.write().await;
            state
                .pending_user_traffic_cleanup
                .entry(node_id.to_string())
                .or_default()
                .insert(user_id.to_string())
        };
        if changed {
            self.persist().await;
        }
    }

    async fn pending_user_traffic_cleanup(&self, node_id: &str) -> Vec<String> {
        let state = self.inner.read().await;
        state
            .pending_user_traffic_cleanup
            .get(node_id)
            .into_iter()
            .flat_map(|users| users.iter().cloned())
            .collect()
    }

    pub async fn complete_user_traffic_cleanup(&self, node_id: &str, user_id: &str) {
        let changed = {
            let mut state = self.inner.write().await;
            let Some(users) = state.pending_user_traffic_cleanup.get_mut(node_id) else {
                return;
            };
            let changed = users.remove(user_id);
            if users.is_empty() {
                state.pending_user_traffic_cleanup.remove(node_id);
            }
            changed
        };
        if changed {
            self.persist().await;
        }
    }

    pub async fn queue_node_history_cleanup(&self, destination_node_id: &str, node_id: &str) {
        let changed = {
            let mut state = self.inner.write().await;
            state
                .pending_node_history_cleanup
                .entry(destination_node_id.to_string())
                .or_default()
                .insert(node_id.to_string())
        };
        if changed {
            self.persist().await;
        }
    }

    async fn pending_node_history_cleanup(&self, destination_node_id: &str) -> Vec<String> {
        let state = self.inner.read().await;
        state
            .pending_node_history_cleanup
            .get(destination_node_id)
            .into_iter()
            .flat_map(|nodes| nodes.iter().cloned())
            .collect()
    }

    pub async fn complete_node_history_cleanup(&self, destination_node_id: &str, node_id: &str) {
        let changed = {
            let mut state = self.inner.write().await;
            let Some(nodes) = state
                .pending_node_history_cleanup
                .get_mut(destination_node_id)
            else {
                return;
            };
            let changed = nodes.remove(node_id);
            if nodes.is_empty() {
                state
                    .pending_node_history_cleanup
                    .remove(destination_node_id);
            }
            changed
        };
        if changed {
            self.persist().await;
        }
    }

    pub async fn node_traffic_report(
        &self,
        node_id: &str,
        window: TrafficWindow,
        now: DateTime<Utc>,
    ) -> Option<TrafficReport> {
        let state = self.inner.read().await;
        let record = state.nodes.get(node_id)?;
        Some(build_traffic_report(&record.traffic_rollup, window, now))
    }

    pub async fn user_traffic_report(
        &self,
        user_id: &str,
        node_id: Option<&str>,
        window: TrafficWindow,
        now: DateTime<Utc>,
    ) -> Option<TrafficReport> {
        let state = self.inner.read().await;
        let mut selected: Vec<&PersistedUserTrafficRecord> = Vec::new();
        for record in state.nodes.values() {
            if node_id.is_some_and(|id| id != record.node_id) {
                continue;
            }
            if let Some(user) = record.user_traffic.get(user_id) {
                selected.push(user);
            }
        }
        if selected.is_empty() {
            return None;
        }
        let aggregate = aggregate_user_records(&selected);
        Some(build_traffic_report(&aggregate, window, now))
    }

    pub async fn user_traffic_node_ids(&self, user_id: &str) -> BTreeSet<String> {
        let state = self.inner.read().await;
        state
            .nodes
            .iter()
            .filter(|(_, record)| {
                record.user_traffic.contains_key(user_id)
                    || record.user_traffic_users.contains(user_id)
            })
            .map(|(node_id, _)| node_id.clone())
            .collect()
    }

    pub async fn mark_sync_error(&self, now: DateTime<Utc>, node_id: &str, error: String) {
        let mut should_persist = false;
        {
            let mut state = self.inner.write().await;
            if let Some(record) = state.nodes.get_mut(node_id) {
                record.last_sync_error = Some(error);
                record.prune(now);
                should_persist = true;
            }
        }
        if should_persist {
            self.persist().await;
        }
    }

    async fn persist(&self) {
        let _guard = self.persistence_lock.lock().await;
        let state = self.inner.read().await.clone();
        if let Err(err) = persist_history_cache(&self.persistence_path, &state) {
            warn!(
                error = %err,
                path = %self.persistence_path.display(),
                "persist node history cache"
            );
        }
    }
}

#[derive(Debug, Clone)]
pub struct NodeTrafficTotals {
    pub membership_key: String,
    pub user_id: Option<String>,
    pub is_probe: bool,
    pub uplink_total: u64,
    pub downlink_total: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrafficWindow {
    Hours24,
    Days31,
}

impl TrafficWindow {
    pub fn parse(value: Option<&str>) -> Result<Self, &'static str> {
        match value.unwrap_or("24h") {
            "24h" => Ok(Self::Hours24),
            "31d" => Ok(Self::Days31),
            _ => Err("window must be 24h or 31d"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hours24 => "24h",
            Self::Days31 => "31d",
        }
    }
}

fn record_traffic_sample(
    record: &mut PersistedNodeHistoryRecord,
    now: DateTime<Utc>,
    sample: NodeTrafficSample,
    deleted_users: &BTreeSet<String>,
) {
    let now_str = rfc3339(now);
    let bucket_start = floor_five_minute(now) - ChronoDuration::seconds(TRAFFIC_ROLLUP_BUCKET_SECS);
    let bucket_end = bucket_start + ChronoDuration::seconds(TRAFFIC_ROLLUP_BUCKET_SECS);
    let mut node_up = 0u64;
    let mut node_down = 0u64;
    let mut node_known =
        sample.totals.is_empty() && sample.unavailable_users.is_empty() && sample.complete;
    let mut warnings = sample.warnings.clone();
    let mut complete = sample.complete;
    let mut gap_dates = BTreeSet::new();

    let mut user_deltas = BTreeMap::<String, UserTrafficDelta>::new();
    let mut saw_new_baseline = false;
    for user_id in &sample.unavailable_users {
        if deleted_users.contains(user_id) {
            continue;
        }
        let entry = user_deltas.entry(user_id.clone()).or_default();
        entry.complete = false;
        entry
            .warnings
            .push("traffic sample unavailable for user".to_string());
    }
    for totals in sample.totals {
        let deleted_user = totals
            .user_id
            .as_ref()
            .is_some_and(|user_id| deleted_users.contains(user_id));
        if let Some(user_id) = totals.user_id.as_ref()
            && !totals.is_probe
            && !deleted_user
        {
            user_deltas.entry(user_id.clone()).or_default();
        }
        let previous = record.traffic_baselines.get(&totals.membership_key);
        let previous_sample_at =
            previous.and_then(|previous| previous.updated_at.parse::<DateTime<Utc>>().ok());
        let previous_is_contiguous =
            previous_sample_at.map(floor_five_minute) == Some(bucket_start);
        let delta = previous
            .filter(|_| previous_is_contiguous)
            .and_then(|previous| {
                (totals.uplink_total >= previous.uplink_total
                    && totals.downlink_total >= previous.downlink_total)
                    .then(|| {
                        (
                            totals.uplink_total - previous.uplink_total,
                            totals.downlink_total - previous.downlink_total,
                        )
                    })
            });
        if let Some((uplink, downlink)) = delta {
            node_up = node_up.saturating_add(uplink);
            node_down = node_down.saturating_add(downlink);
            node_known = true;
            if let Some(user_id) = totals.user_id.as_ref()
                && !totals.is_probe
                && !deleted_user
            {
                let entry = user_deltas.entry(user_id.clone()).or_default();
                entry.uplink = entry.uplink.saturating_add(uplink);
                entry.downlink = entry.downlink.saturating_add(downlink);
                entry.known = true;
            }
        } else if previous.is_none() {
            complete = false;
            saw_new_baseline = true;
            if let Some(user_id) = totals.user_id.as_ref()
                && !totals.is_probe
                && !deleted_user
            {
                let entry = user_deltas.entry(user_id.clone()).or_default();
                entry.complete = false;
                entry.warnings.push(format!(
                    "traffic tracking started; first sample has no delta for {}",
                    totals.membership_key
                ));
            }
        } else if !previous_is_contiguous {
            complete = false;
            if let Some(previous_sample_at) = previous_sample_at {
                let previous_date = date_key(previous_sample_at);
                gap_dates.insert(previous_date.clone());
                if let Some(user_id) = totals.user_id.as_ref()
                    && !totals.is_probe
                    && !deleted_user
                {
                    let entry = user_deltas.entry(user_id.clone()).or_default();
                    entry.gap_dates.insert(previous_date);
                }
            }
            warnings.push(format!(
                "sampling gap for {}; recovered bucket has no delta",
                totals.membership_key
            ));
            if let Some(user_id) = totals.user_id.as_ref()
                && !totals.is_probe
                && !deleted_user
            {
                let entry = user_deltas.entry(user_id.clone()).or_default();
                entry.complete = false;
                entry.warnings.push(format!(
                    "sampling gap for {}; recovered bucket has no delta",
                    totals.membership_key
                ));
            }
        } else if previous.is_some() {
            complete = false;
            warnings.push(format!(
                "counter reset or decreased for {}",
                totals.membership_key
            ));
            if let Some(user_id) = totals.user_id.as_ref()
                && !totals.is_probe
                && !deleted_user
            {
                let entry = user_deltas.entry(user_id.clone()).or_default();
                entry.complete = false;
                entry.warnings.push(format!(
                    "counter reset or decreased for {}",
                    totals.membership_key
                ));
            }
        }
        record.traffic_baselines.insert(
            totals.membership_key,
            TrafficBaseline {
                uplink_total: totals.uplink_total,
                downlink_total: totals.downlink_total,
                updated_at: now_str.clone(),
            },
        );
    }

    if saw_new_baseline {
        warnings.push("traffic tracking started; first sample has no delta".to_string());
    }

    let node_complete = complete && node_known;
    let node_bucket = NodeTrafficBucket {
        start_at: rfc3339(bucket_start),
        end_at: rfc3339(bucket_end),
        uplink_bytes: node_complete.then_some(node_up),
        downlink_bytes: node_complete.then_some(node_down),
        complete: node_complete,
        warnings: warnings.clone(),
    };
    upsert_five_minute_bucket(&mut record.traffic_rollup.five_minute, node_bucket);
    record.traffic_rollup.last_sample_at = Some(now_str.clone());
    update_daily_rollup(
        &mut record.traffic_rollup.daily,
        bucket_start,
        node_complete.then_some(node_up),
        node_complete.then_some(node_down),
        node_complete,
        warnings.clone(),
    );
    for date in gap_dates {
        mark_daily_rollup_incomplete(
            &mut record.traffic_rollup.daily,
            &date,
            format!("sampling gap before {date}"),
        );
    }
    update_legacy_daily_traffic(record, bucket_start, now, node_up, node_down, node_known);
    update_cycle_accumulator(
        &mut record.traffic_rollup.cycle,
        sample.cycle.as_ref(),
        node_complete.then_some(node_up),
        node_complete.then_some(node_down),
        node_complete,
        &warnings,
        &now_str,
    );

    for (user_id, delta) in &user_deltas {
        let user = record.user_traffic.entry(user_id.clone()).or_default();
        user.membership_active = true;
        let user_complete = delta.complete && delta.known;
        let user_bucket = NodeTrafficBucket {
            start_at: rfc3339(bucket_start),
            end_at: rfc3339(bucket_end),
            uplink_bytes: user_complete.then_some(delta.uplink),
            downlink_bytes: user_complete.then_some(delta.downlink),
            complete: user_complete,
            warnings: delta.warnings.clone(),
        };
        upsert_five_minute_bucket(&mut user.five_minute, user_bucket);
        user.last_sample_at = Some(now_str.clone());
        update_daily_rollup(
            &mut user.daily,
            bucket_start,
            user_complete.then_some(delta.uplink),
            user_complete.then_some(delta.downlink),
            user_complete,
            delta.warnings.clone(),
        );
        for date in &delta.gap_dates {
            mark_daily_rollup_incomplete(
                &mut user.daily,
                date,
                format!("sampling gap before {date}"),
            );
        }
        update_cycle_accumulator(
            &mut user.cycle,
            sample.user_cycles.get(user_id),
            user_complete.then_some(delta.uplink),
            user_complete.then_some(delta.downlink),
            user_complete,
            &delta.warnings,
            &now_str,
        );
    }

    // Record one incomplete transition after a membership is removed, then let the
    // retained buckets expire without refreshing the inactive record forever.
    for (user_id, context) in sample.user_cycles {
        if deleted_users.contains(&user_id) {
            continue;
        }
        if user_deltas.contains_key(&user_id) {
            continue;
        }
        if let Some(user) = record.user_traffic.get_mut(&user_id) {
            if !user.membership_active {
                continue;
            }
            let warning = "membership removed; bucket has no delta".to_string();
            user.membership_active = false;
            let (uplink, downlink, complete, warnings) = (None, None, false, vec![warning.clone()]);
            upsert_five_minute_bucket(
                &mut user.five_minute,
                NodeTrafficBucket {
                    start_at: rfc3339(bucket_start),
                    end_at: rfc3339(bucket_end),
                    uplink_bytes: uplink,
                    downlink_bytes: downlink,
                    complete,
                    warnings: warnings.clone(),
                },
            );
            user.last_sample_at = Some(now_str.clone());
            update_daily_rollup(
                &mut user.daily,
                bucket_start,
                uplink,
                downlink,
                complete,
                warnings.clone(),
            );
            update_cycle_accumulator(
                &mut user.cycle,
                Some(&context),
                uplink,
                downlink,
                complete,
                &warnings,
                &now_str,
            );
        }
    }
}

fn update_legacy_daily_traffic(
    record: &mut PersistedNodeHistoryRecord,
    bucket_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    uplink: u64,
    downlink: u64,
    known: bool,
) {
    let date = date_key(bucket_at);
    if !known && !record.daily_traffic.contains_key(&date) {
        return;
    }
    let entry =
        record
            .daily_traffic
            .entry(date.clone())
            .or_insert_with(|| NodeHistoryDailyTraffic {
                date,
                uplink_bytes: 0,
                downlink_bytes: 0,
                updated_at: rfc3339(updated_at),
            });
    if known {
        entry.uplink_bytes = entry.uplink_bytes.saturating_add(uplink);
        entry.downlink_bytes = entry.downlink_bytes.saturating_add(downlink);
    }
    entry.updated_at = rfc3339(updated_at);
}

fn upsert_five_minute_bucket(buckets: &mut Vec<NodeTrafficBucket>, bucket: NodeTrafficBucket) {
    if let Some(existing) = buckets
        .iter_mut()
        .find(|item| item.start_at == bucket.start_at)
    {
        *existing = bucket;
    } else {
        buckets.push(bucket);
    }
    buckets.sort_by(|a, b| a.start_at.cmp(&b.start_at));
}

fn update_daily_rollup(
    buckets: &mut Vec<NodeTrafficDailyBucket>,
    now: DateTime<Utc>,
    uplink: Option<u64>,
    downlink: Option<u64>,
    complete: bool,
    warnings: Vec<String>,
) {
    let date = date_key(now);
    let entry = if let Some(entry) = buckets.iter_mut().find(|entry| entry.date == date) {
        entry
    } else {
        buckets.push(NodeTrafficDailyBucket {
            date: date.clone(),
            uplink_bytes: None,
            downlink_bytes: None,
            complete: true,
            warnings: Vec::new(),
        });
        buckets.last_mut().expect("daily bucket was just inserted")
    };
    if complete && entry.complete {
        if let Some(value) = uplink {
            entry.uplink_bytes = Some(entry.uplink_bytes.unwrap_or(0).saturating_add(value));
        }
        if let Some(value) = downlink {
            entry.downlink_bytes = Some(entry.downlink_bytes.unwrap_or(0).saturating_add(value));
        }
    } else {
        entry.uplink_bytes = None;
        entry.downlink_bytes = None;
    }
    entry.complete &= complete;
    entry.warnings.extend(warnings);
    entry.warnings.sort();
    entry.warnings.dedup();
    buckets.sort_by(|a, b| a.date.cmp(&b.date));
}

fn mark_daily_rollup_incomplete(
    buckets: &mut Vec<NodeTrafficDailyBucket>,
    date: &str,
    warning: String,
) {
    let entry = if let Some(entry) = buckets.iter_mut().find(|entry| entry.date == date) {
        entry
    } else {
        buckets.push(NodeTrafficDailyBucket {
            date: date.to_string(),
            uplink_bytes: None,
            downlink_bytes: None,
            complete: false,
            warnings: Vec::new(),
        });
        buckets.last_mut().expect("daily bucket was just inserted")
    };
    entry.complete = false;
    entry.uplink_bytes = None;
    entry.downlink_bytes = None;
    entry.warnings.push(warning);
    entry.warnings.sort();
    entry.warnings.dedup();
    buckets.sort_by(|a, b| a.date.cmp(&b.date));
}

fn update_cycle_accumulator(
    accumulator: &mut Option<TrafficCycleAccumulator>,
    context: Option<&TrafficCycleContext>,
    uplink: Option<u64>,
    downlink: Option<u64>,
    complete: bool,
    warnings: &[String],
    sampled_at: &str,
) {
    let Some(context) = context else { return };
    let mode = match context.mode {
        TrafficCycleMode::Monthly => "monthly",
        TrafficCycleMode::Unlimited => "unlimited",
    };
    let reset = accumulator.as_ref().is_none_or(|current| {
        current.start_at != context.start_at || current.end_at != context.end_at
    });
    let configuration_changed = accumulator.as_ref().is_some_and(|current| {
        current.mode != mode || (mode == "monthly" && current.end_at != context.start_at)
    });
    let had_accumulator = accumulator.is_some();
    if reset {
        let warning = if configuration_changed {
            "quota cycle configuration changed; traffic accumulator reset"
        } else if accumulator.is_some() {
            "quota cycle changed; traffic accumulator reset"
        } else {
            "traffic tracking started; prior cycle usage is unavailable"
        };
        *accumulator = Some(TrafficCycleAccumulator {
            mode: mode.to_string(),
            start_at: context.start_at.clone(),
            end_at: context.end_at.clone(),
            uplink_bytes: 0,
            downlink_bytes: 0,
            complete: had_accumulator
                && !configuration_changed
                && complete
                && uplink.is_some()
                && downlink.is_some(),
            tracking_since: sampled_at.to_string(),
            warnings: vec![warning.to_string()],
        });
    }
    let Some(accumulator) = accumulator.as_mut() else {
        return;
    };
    if let Some(value) = uplink {
        accumulator.uplink_bytes = accumulator.uplink_bytes.saturating_add(value);
    }
    if let Some(value) = downlink {
        accumulator.downlink_bytes = accumulator.downlink_bytes.saturating_add(value);
    }
    accumulator.complete &= complete && uplink.is_some() && downlink.is_some();
    accumulator.warnings.extend(warnings.iter().cloned());
    accumulator.warnings.sort();
    accumulator.warnings.dedup();
}

fn floor_five_minute(at: DateTime<Utc>) -> DateTime<Utc> {
    let timestamp = at.timestamp();
    let floored = timestamp - timestamp.rem_euclid(TRAFFIC_ROLLUP_BUCKET_SECS);
    DateTime::from_timestamp(floored, 0).unwrap_or(at)
}

fn aggregate_user_records(records: &[&PersistedUserTrafficRecord]) -> NodeTrafficRollupSnapshot {
    let mut five_minute = BTreeMap::<String, NodeTrafficBucket>::new();
    let mut daily = BTreeMap::<String, NodeTrafficDailyBucket>::new();
    let mut cycle: Option<TrafficCycleAccumulator> = None;
    let mut last_sample_at: Option<String> = None;

    for record in records {
        for bucket in &record.five_minute {
            if let Some(entry) = five_minute.get_mut(&bucket.start_at) {
                entry.uplink_bytes = add_required(entry.uplink_bytes, bucket.uplink_bytes);
                entry.downlink_bytes = add_required(entry.downlink_bytes, bucket.downlink_bytes);
                entry.complete &= bucket.complete
                    && entry.uplink_bytes.is_some()
                    && entry.downlink_bytes.is_some();
                entry.warnings.extend(bucket.warnings.clone());
            } else {
                five_minute.insert(bucket.start_at.clone(), bucket.clone());
            }
        }
        for bucket in &record.daily {
            if let Some(entry) = daily.get_mut(&bucket.date) {
                entry.uplink_bytes = add_required(entry.uplink_bytes, bucket.uplink_bytes);
                entry.downlink_bytes = add_required(entry.downlink_bytes, bucket.downlink_bytes);
                entry.complete &= bucket.complete
                    && entry.uplink_bytes.is_some()
                    && entry.downlink_bytes.is_some();
                entry.warnings.extend(bucket.warnings.clone());
            } else {
                daily.insert(bucket.date.clone(), bucket.clone());
            }
        }
        if let Some(current) = &record.cycle {
            let has_active_cycle = records.iter().any(|candidate| {
                candidate.membership_active
                    && candidate.cycle.as_ref().is_some_and(|cycle| {
                        cycle.start_at == current.start_at && cycle.end_at == current.end_at
                    })
            });
            if !record.membership_active
                && records
                    .iter()
                    .any(|candidate| candidate.membership_active && candidate.cycle.is_some())
                && !has_active_cycle
            {
                continue;
            }
            if let Some(existing) = cycle.as_mut() {
                if existing.start_at == current.start_at && existing.end_at == current.end_at {
                    existing.uplink_bytes =
                        existing.uplink_bytes.saturating_add(current.uplink_bytes);
                    existing.downlink_bytes = existing
                        .downlink_bytes
                        .saturating_add(current.downlink_bytes);
                    existing.complete &= current.complete;
                    existing.warnings.extend(current.warnings.clone());
                } else {
                    existing.complete = false;
                    existing
                        .warnings
                        .push("cycle configuration differs across nodes".to_string());
                }
            } else {
                cycle = Some(current.clone());
            }
        }
        if record.last_sample_at.as_deref() > last_sample_at.as_deref() {
            last_sample_at = record.last_sample_at.clone();
        }
    }
    for (start_at, bucket) in &mut five_minute {
        if records.iter().any(|record| {
            five_minute_bucket_is_applicable(record, start_at)
                && !record
                    .five_minute
                    .iter()
                    .any(|candidate| candidate.start_at == *start_at)
        }) {
            bucket.uplink_bytes = None;
            bucket.downlink_bytes = None;
            bucket.complete = false;
            bucket
                .warnings
                .push("sampling gap across aggregated nodes".to_string());
        }
    }
    for (date, bucket) in &mut daily {
        if records.iter().any(|record| {
            daily_bucket_is_applicable(record, date)
                && !record.daily.iter().any(|candidate| candidate.date == *date)
        }) {
            bucket.uplink_bytes = None;
            bucket.downlink_bytes = None;
            bucket.complete = false;
            bucket
                .warnings
                .push("sampling gap across aggregated nodes".to_string());
        }
    }

    let mut five_minute = five_minute.into_values().collect::<Vec<_>>();
    five_minute.sort_by(|a, b| a.start_at.cmp(&b.start_at));
    let mut daily = daily.into_values().collect::<Vec<_>>();
    daily.sort_by(|a, b| a.date.cmp(&b.date));
    if let Some(cycle) = cycle.as_mut() {
        cycle.warnings.sort();
        cycle.warnings.dedup();
    }
    NodeTrafficRollupSnapshot {
        five_minute,
        daily,
        cycle,
        last_sample_at,
    }
}

fn five_minute_bucket_is_applicable(record: &PersistedUserTrafficRecord, start_at: &str) -> bool {
    let Some(first) = record.five_minute.first() else {
        return false;
    };
    if start_at < first.start_at.as_str() {
        return false;
    }
    record.membership_active
        || record
            .five_minute
            .last()
            .is_some_and(|last| start_at <= last.start_at.as_str())
}

fn daily_bucket_is_applicable(record: &PersistedUserTrafficRecord, date: &str) -> bool {
    let Some(first) = record.daily.first() else {
        return false;
    };
    if date < first.date.as_str() {
        return false;
    }
    record.membership_active
        || record
            .daily
            .last()
            .is_some_and(|last| date <= last.date.as_str())
}

fn add_required(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.saturating_add(right)),
        (_, _) => None,
    }
}

fn build_traffic_report(
    rollup: &NodeTrafficRollupSnapshot,
    window: TrafficWindow,
    now: DateTime<Utc>,
) -> TrafficReport {
    let latest_sample = floor_five_minute(now);
    let (window_start, window_end, current, reference) = match window {
        TrafficWindow::Hours24 => {
            let end = latest_sample;
            let start = end - ChronoDuration::seconds(288 * TRAFFIC_ROLLUP_BUCKET_SECS);
            let reference_start = start - ChronoDuration::seconds(288 * TRAFFIC_ROLLUP_BUCKET_SECS);
            let current = (0..288)
                .map(|index| {
                    let bucket_start =
                        start + ChronoDuration::seconds(index * TRAFFIC_ROLLUP_BUCKET_SECS);
                    traffic_point_from_five_minute(
                        rollup
                            .five_minute
                            .iter()
                            .find(|bucket| bucket.start_at == rfc3339(bucket_start)),
                        bucket_start,
                        false,
                    )
                })
                .collect::<Vec<_>>();
            let reference = (0..288)
                .map(|index| {
                    let bucket_start = reference_start
                        + ChronoDuration::seconds(index * TRAFFIC_ROLLUP_BUCKET_SECS);
                    traffic_point_from_five_minute(
                        rollup
                            .five_minute
                            .iter()
                            .find(|bucket| bucket.start_at == rfc3339(bucket_start)),
                        bucket_start,
                        false,
                    )
                })
                .collect::<Vec<_>>();
            (start, end, current, reference)
        }
        TrafficWindow::Days31 => {
            let current_date = latest_sample.date_naive();
            let start_date = current_date - chrono::Days::new(30);
            let start = start_date.and_hms_opt(0, 0, 0).unwrap().and_utc();
            let end = (current_date + chrono::Days::new(1))
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc();
            let reference_start = start - ChronoDuration::days(31);
            let current = (0..31)
                .map(|index| {
                    let date = start_date + chrono::Days::new(index as u64);
                    traffic_point_from_daily(
                        rollup
                            .daily
                            .iter()
                            .find(|bucket| bucket.date == date.to_string()),
                        date,
                        date == current_date,
                    )
                })
                .collect::<Vec<_>>();
            let reference = (0..31)
                .map(|index| {
                    let date = reference_start.date_naive() + chrono::Days::new(index as u64);
                    traffic_point_from_daily(
                        rollup
                            .daily
                            .iter()
                            .find(|bucket| bucket.date == date.to_string()),
                        date,
                        false,
                    )
                })
                .collect::<Vec<_>>();
            (start, end, current, reference)
        }
    };

    let sample_is_stale = rollup
        .last_sample_at
        .as_deref()
        .and_then(|sampled_at| sampled_at.parse::<DateTime<Utc>>().ok())
        .is_none_or(|sampled_at| floor_five_minute(sampled_at) < latest_sample);
    let mut summary = build_summary(rollup, latest_sample);
    let mut warnings = summary_warnings(rollup, &current, &reference, window);
    if sample_is_stale {
        summary.complete = false;
        warnings.push("traffic sample is stale at the current UTC boundary".to_string());
    }
    let mut current = current;
    if sample_is_stale
        && window == TrafficWindow::Days31
        && let Some(point) = current.iter_mut().find(|point| point.is_current_day)
    {
        point.uplink_bytes = None;
        point.downlink_bytes = None;
        point.total_bytes = None;
        point.complete = false;
    }
    warnings.sort();
    warnings.dedup();
    let partial = !summary.complete
        || current
            .iter()
            .any(|point| point.uplink_bytes.is_none() || !point.complete)
        || reference
            .iter()
            .any(|point| point.uplink_bytes.is_none() || !point.complete);
    TrafficReport {
        window: window.as_str().to_string(),
        window_start_at: rfc3339(window_start),
        window_end_at: rfc3339(window_end),
        timezone: "UTC".to_string(),
        summary,
        current,
        reference: Some(reference),
        partial,
        last_sample_at: rollup.last_sample_at.clone(),
        warnings,
    }
}

fn traffic_point_from_five_minute(
    bucket: Option<&NodeTrafficBucket>,
    start: DateTime<Utc>,
    is_current_day: bool,
) -> TrafficSeriesPoint {
    let end = start + ChronoDuration::seconds(TRAFFIC_ROLLUP_BUCKET_SECS);
    let complete_bucket = bucket.filter(|bucket| bucket.complete);
    traffic_point(
        rfc3339(start),
        rfc3339(end),
        complete_bucket.and_then(|bucket| bucket.uplink_bytes),
        complete_bucket.and_then(|bucket| bucket.downlink_bytes),
        complete_bucket.is_some(),
        is_current_day,
    )
}

fn traffic_point_from_daily(
    bucket: Option<&NodeTrafficDailyBucket>,
    date: chrono::NaiveDate,
    is_current_day: bool,
) -> TrafficSeriesPoint {
    let start = date.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end = (date + chrono::Days::new(1))
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc();
    let complete_bucket = bucket.filter(|bucket| bucket.complete);
    traffic_point(
        rfc3339(start),
        rfc3339(end),
        complete_bucket.and_then(|bucket| bucket.uplink_bytes),
        complete_bucket.and_then(|bucket| bucket.downlink_bytes),
        complete_bucket.is_some(),
        is_current_day,
    )
}

fn traffic_point(
    start_at: String,
    end_at: String,
    uplink_bytes: Option<u64>,
    downlink_bytes: Option<u64>,
    complete: bool,
    is_current_day: bool,
) -> TrafficSeriesPoint {
    TrafficSeriesPoint {
        start_at,
        end_at,
        total_bytes: uplink_bytes
            .zip(downlink_bytes)
            .map(|(up, down)| up.saturating_add(down)),
        uplink_bytes,
        downlink_bytes,
        complete,
        is_current_day,
    }
}

fn build_summary(
    rollup: &NodeTrafficRollupSnapshot,
    latest_sample: DateTime<Utc>,
) -> TrafficSummary {
    if let Some(cycle) = &rollup.cycle
        && cycle.mode == "monthly"
    {
        return TrafficSummary {
            mode: "cycle".to_string(),
            cycle_start_at: Some(cycle.start_at.clone()),
            cycle_end_at: Some(cycle.end_at.clone()),
            uplink_bytes: cycle.uplink_bytes,
            downlink_bytes: cycle.downlink_bytes,
            total_bytes: cycle.uplink_bytes.saturating_add(cycle.downlink_bytes),
            complete: cycle.complete,
            tracking_since: Some(cycle.tracking_since.clone()),
        };
    }
    let cutoff = latest_sample.date_naive() - chrono::Days::new(29);
    let mut uplink = 0u64;
    let mut downlink = 0u64;
    let mut complete = true;
    let mut tracking_since = None;
    for index in 0..30 {
        let date = cutoff + chrono::Days::new(index);
        let bucket = rollup
            .daily
            .iter()
            .find(|bucket| bucket.date == date.to_string());
        let Some(bucket) = bucket else {
            complete = false;
            continue;
        };
        if let Some(value) = bucket.uplink_bytes {
            uplink = uplink.saturating_add(value);
        }
        if let Some(value) = bucket.downlink_bytes {
            downlink = downlink.saturating_add(value);
        }
        complete &=
            bucket.complete && bucket.uplink_bytes.is_some() && bucket.downlink_bytes.is_some();
        if tracking_since.is_none() {
            tracking_since = chrono::NaiveDate::parse_from_str(&bucket.date, "%Y-%m-%d")
                .ok()
                .and_then(|date| date.and_hms_opt(0, 0, 0))
                .map(|date| rfc3339(date.and_utc()));
        }
    }
    TrafficSummary {
        mode: "rolling_30d".to_string(),
        cycle_start_at: Some(rfc3339(
            (latest_sample.date_naive() - chrono::Days::new(29))
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc(),
        )),
        cycle_end_at: Some(rfc3339(
            (latest_sample.date_naive() + chrono::Days::new(1))
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc(),
        )),
        uplink_bytes: uplink,
        downlink_bytes: downlink,
        total_bytes: uplink.saturating_add(downlink),
        complete,
        tracking_since,
    }
}

fn summary_warnings(
    rollup: &NodeTrafficRollupSnapshot,
    current: &[TrafficSeriesPoint],
    reference: &[TrafficSeriesPoint],
    window: TrafficWindow,
) -> Vec<String> {
    let mut warnings = Vec::new();
    match window {
        TrafficWindow::Hours24 => {
            let starts = current
                .iter()
                .chain(reference.iter())
                .map(|point| point.start_at.as_str())
                .collect::<BTreeSet<_>>();
            warnings.extend(
                rollup
                    .five_minute
                    .iter()
                    .filter(|bucket| starts.contains(bucket.start_at.as_str()))
                    .flat_map(|bucket| bucket.warnings.clone()),
            );
        }
        TrafficWindow::Days31 => {
            let dates = current
                .iter()
                .chain(reference.iter())
                .filter_map(|point| point.start_at.get(..10))
                .collect::<BTreeSet<_>>();
            warnings.extend(
                rollup
                    .daily
                    .iter()
                    .filter(|bucket| dates.contains(bucket.date.as_str()))
                    .flat_map(|bucket| bucket.warnings.clone()),
            );
        }
    }
    warnings.extend(rollup.cycle.iter().flat_map(|cycle| cycle.warnings.clone()));
    if current.iter().any(|point| point.uplink_bytes.is_none()) {
        warnings.push("sampling gap in current window".to_string());
    }
    if reference.iter().any(|point| point.uplink_bytes.is_none()) {
        warnings.push("sampling gap in reference window".to_string());
    }
    warnings
}

pub fn merge_traffic_reports(
    reports: &[TrafficReport],
    window: TrafficWindow,
    now: DateTime<Utc>,
) -> TrafficReport {
    if reports.is_empty() {
        return build_traffic_report(&NodeTrafficRollupSnapshot::default(), window, now);
    }
    let first = &reports[0];
    let window_end = reports
        .iter()
        .filter_map(|report| report.window_end_at.parse::<DateTime<Utc>>().ok())
        .max()
        .unwrap_or_else(|| floor_five_minute(now));
    let (bucket_count, bucket_duration) = match window {
        TrafficWindow::Hours24 => (288usize, ChronoDuration::hours(24)),
        TrafficWindow::Days31 => (31usize, ChronoDuration::days(31)),
    };
    let bucket_step = match window {
        TrafficWindow::Hours24 => ChronoDuration::minutes(5),
        TrafficWindow::Days31 => ChronoDuration::days(1),
    };
    let window_start = window_end - bucket_duration;
    let reference_start = window_start - bucket_duration;
    let point_at = |start: DateTime<Utc>, is_reference: bool| -> TrafficSeriesPoint {
        let key = rfc3339(start);
        let mut uplink = Some(0u64);
        let mut downlink = Some(0u64);
        let mut complete = true;
        for report in reports {
            let point = if is_reference {
                report
                    .reference
                    .as_ref()
                    .and_then(|points| points.iter().find(|point| point.start_at == key))
            } else {
                report.current.iter().find(|point| point.start_at == key)
            };
            let Some(point) = point else {
                complete = false;
                uplink = None;
                downlink = None;
                continue;
            };
            if !point.complete {
                complete = false;
                uplink = None;
                downlink = None;
                continue;
            }
            uplink = add_required(uplink, point.uplink_bytes);
            downlink = add_required(downlink, point.downlink_bytes);
        }
        let is_current_day = matches!(window, TrafficWindow::Days31)
            && start.date_naive() == (window_end - ChronoDuration::days(1)).date_naive();
        traffic_point(
            rfc3339(start),
            rfc3339(start + bucket_step),
            uplink,
            downlink,
            complete && uplink.is_some() && downlink.is_some(),
            is_current_day,
        )
    };
    let current = (0..bucket_count)
        .map(|index| point_at(window_start + bucket_step * index as i32, false))
        .collect::<Vec<_>>();
    let reference = reports
        .iter()
        .any(|report| report.reference.is_some())
        .then(|| {
            (0..bucket_count)
                .map(|index| point_at(reference_start + bucket_step * index as i32, true))
                .collect::<Vec<_>>()
        });
    let summaries_compatible = reports.iter().all(|report| {
        report.summary.mode == first.summary.mode
            && report.summary.cycle_start_at == first.summary.cycle_start_at
            && report.summary.cycle_end_at == first.summary.cycle_end_at
    });
    let mut summary = first.summary.clone();
    if summaries_compatible {
        summary.uplink_bytes = reports.iter().fold(0u64, |sum, report| {
            sum.saturating_add(report.summary.uplink_bytes)
        });
        summary.downlink_bytes = reports.iter().fold(0u64, |sum, report| {
            sum.saturating_add(report.summary.downlink_bytes)
        });
        summary.total_bytes = summary.uplink_bytes.saturating_add(summary.downlink_bytes);
        summary.complete = reports.iter().all(|report| report.summary.complete);
    } else {
        summary.cycle_start_at = None;
        summary.cycle_end_at = None;
        summary.uplink_bytes = 0;
        summary.downlink_bytes = 0;
        summary.total_bytes = 0;
        summary.complete = false;
        summary.tracking_since = None;
    }
    let mut warnings = reports
        .iter()
        .flat_map(|report| report.warnings.clone())
        .collect::<Vec<_>>();
    if !summaries_compatible {
        warnings.push("traffic summaries span different quota cycles".to_string());
    }
    if current
        .iter()
        .any(|point| point.uplink_bytes.is_none() || !point.complete)
    {
        warnings.push("sampling gap in current window".to_string());
    }
    if reference.as_ref().is_some_and(|points| {
        points
            .iter()
            .any(|point| point.uplink_bytes.is_none() || !point.complete)
    }) {
        warnings.push("sampling gap in reference window".to_string());
    }
    warnings.sort();
    warnings.dedup();
    let partial = !summary.complete
        || current
            .iter()
            .any(|point| point.uplink_bytes.is_none() || !point.complete)
        || reference.as_ref().is_some_and(|points| {
            points
                .iter()
                .any(|point| point.uplink_bytes.is_none() || !point.complete)
        });
    TrafficReport {
        window: window.as_str().to_string(),
        window_start_at: rfc3339(window_start),
        window_end_at: rfc3339(window_end),
        timezone: "UTC".to_string(),
        summary,
        current,
        reference,
        partial,
        last_sample_at: reports
            .iter()
            .filter_map(|report| report.last_sample_at.clone())
            .max(),
        warnings,
    }
}

fn record_daily_components(
    record: &mut PersistedNodeHistoryRecord,
    now: DateTime<Utc>,
    runtime: LocalNodeRuntimeSnapshot,
) {
    let now_str = rfc3339(now);
    let date = date_key(now);
    let mut components = runtime
        .components
        .into_iter()
        .map(|component| NodeHistoryComponentDayStatus {
            component: component.component,
            status: component.status,
            observed_at: now_str.clone(),
        })
        .collect::<Vec<_>>();
    components.sort_by_key(|a| a.component);
    record.daily_component_status.insert(
        date.clone(),
        NodeHistoryDailyComponentStatus { date, components },
    );

    let mut by_id = record
        .component_status_events
        .iter()
        .map(|event| (event.event_id.clone(), event.clone()))
        .collect::<BTreeMap<_, _>>();
    for event in runtime.events {
        if event.kind != NodeRuntimeEventKind::StatusChanged {
            continue;
        }
        by_id.insert(
            event.event_id.clone(),
            NodeHistoryComponentStatusEvent {
                event_id: event.event_id,
                occurred_at: event.occurred_at,
                component: event.component,
                message: event.message,
                from_status: event.from_status,
                to_status: event.to_status,
            },
        );
    }
    record.component_status_events = by_id.into_values().collect();
}

pub fn spawn_node_history_local_worker(
    config: Arc<Config>,
    local_node_id: String,
    store: Arc<Mutex<JsonSnapshotStore>>,
    runtime: NodeRuntimeHandle,
    history: NodeHistoryHandle,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        sleep_until_next_traffic_boundary().await;
        let mut ticker = tokio::time::interval(Duration::from_secs(SYNC_INTERVAL_SECS));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let now = Utc::now();
            let bucket_start =
                floor_five_minute(now) - ChronoDuration::seconds(TRAFFIC_ROLLUP_BUCKET_SECS);
            let collection = collect_local_traffic_totals(&config, &store, &local_node_id).await;
            let (node, users) = {
                let store = store.lock().await;
                (store.get_node(&local_node_id), store.list_users())
            };
            let sample = collection.map(|collection| NodeTrafficSample {
                totals: collection.totals,
                unavailable_users: collection.unavailable_users,
                complete: collection.complete,
                warnings: collection.warnings,
                cycle: node
                    .as_ref()
                    .and_then(|node| traffic_cycle_for_node(node, bucket_start)),
                user_cycles: users
                    .iter()
                    .filter_map(|user| {
                        traffic_cycle_for_user(user, bucket_start)
                            .map(|cycle| (user.user_id.clone(), cycle))
                    })
                    .collect(),
            });
            let runtime_snapshot = runtime.snapshot(MAX_EVENTS_PER_NODE).await;
            history
                .record_local_sample_with_status(now, &local_node_id, sample, runtime_snapshot)
                .await;
        }
    })
}

async fn sleep_until_next_traffic_boundary() {
    let now = Utc::now();
    let remainder = now.timestamp().rem_euclid(TRAFFIC_ROLLUP_BUCKET_SECS);
    let wait_secs = (TRAFFIC_ROLLUP_BUCKET_SECS - remainder).max(1) as u64;
    tokio::time::sleep(Duration::from_secs(wait_secs)).await;
}

pub fn spawn_node_history_remote_sync_worker(
    cluster_id: String,
    local_node_id: String,
    store: Arc<Mutex<JsonSnapshotStore>>,
    history: NodeHistoryHandle,
    cluster_ca_key_pem: String,
    cluster_ca_pem: String,
    mesh_client: MeshAwareHttpClient,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(SYNC_INTERVAL_SECS));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let (nodes, endpoints) = {
                let store = store.lock().await;
                (store.list_nodes(), store.list_endpoints())
            };
            sync_remote_node_histories(
                &mesh_client,
                &remote::RemoteSyncAuth::new(
                    &cluster_id,
                    &local_node_id,
                    &cluster_ca_key_pem,
                    &cluster_ca_pem,
                ),
                &history,
                &local_node_id,
                nodes,
                endpoints,
            )
            .await;
        }
    })
}

struct TrafficCollection {
    totals: Vec<NodeTrafficTotals>,
    unavailable_users: BTreeSet<String>,
    complete: bool,
    warnings: Vec<String>,
}

async fn collect_local_traffic_totals(
    config: &Config,
    store: &Arc<Mutex<JsonSnapshotStore>>,
    local_node_id: &str,
) -> Option<TrafficCollection> {
    let memberships = {
        let store = store.lock().await;
        store
            .state()
            .node_user_endpoint_memberships
            .iter()
            .filter(|membership| membership.node_id == local_node_id)
            .map(|membership| {
                (
                    membership_xray_email(&membership.user_id, &membership.endpoint_id),
                    membership.user_id.clone(),
                    membership.user_id == crate::endpoint_probe::PROBE_USER_ID,
                )
            })
            .collect::<Vec<_>>()
    };
    if memberships.is_empty() {
        return Some(TrafficCollection {
            totals: Vec::new(),
            unavailable_users: BTreeSet::new(),
            complete: true,
            warnings: Vec::new(),
        });
    }

    let mut client = match xray::connect(config.xray_api_addr).await {
        Ok(client) => client,
        Err(err) => {
            warn!(%err, "node history traffic sample skipped: xray connect failed");
            return None;
        }
    };

    let mut out = Vec::new();
    let mut unavailable_users = BTreeSet::new();
    let mut warnings = Vec::new();
    for (email, user_id, is_probe) in memberships.iter() {
        match client.get_user_traffic_totals(email).await {
            Ok((uplink, downlink)) => {
                out.push(NodeTrafficTotals {
                    membership_key: email.clone(),
                    user_id: (!*is_probe).then(|| user_id.clone()),
                    is_probe: *is_probe,
                    uplink_total: uplink,
                    downlink_total: downlink,
                });
            }
            Err(err) => {
                if !*is_probe {
                    unavailable_users.insert(user_id.clone());
                }
                warnings.push(format!("traffic sample unavailable for {email}: {err}"));
                warn!(email, %err, "node history traffic stat skipped");
            }
        }
    }
    Some(TrafficCollection {
        complete: warnings.is_empty(),
        totals: out,
        unavailable_users,
        warnings,
    })
}

fn traffic_cycle_for_node(node: &Node, now: DateTime<Utc>) -> Option<TrafficCycleContext> {
    match node.quota_reset {
        NodeQuotaReset::Unlimited { .. } => {
            let start = (now.date_naive() - chrono::Days::new(29))
                .and_hms_opt(0, 0, 0)?
                .and_utc();
            let end = (now.date_naive() + chrono::Days::new(1))
                .and_hms_opt(0, 0, 0)?
                .and_utc();
            Some(TrafficCycleContext {
                start_at: rfc3339(start),
                end_at: rfc3339(end),
                mode: TrafficCycleMode::Unlimited,
            })
        }
        NodeQuotaReset::Monthly {
            day_of_month,
            tz_offset_minutes,
        } => {
            let tz = tz_offset_minutes
                .map(|tz_offset_minutes| CycleTimeZone::FixedOffsetMinutes { tz_offset_minutes })
                .unwrap_or(CycleTimeZone::Local);
            let (start, end) = current_cycle_window_at(tz, day_of_month, now).ok()?;
            Some(TrafficCycleContext {
                start_at: start.with_timezone(&Utc).to_rfc3339(),
                end_at: end.with_timezone(&Utc).to_rfc3339(),
                mode: TrafficCycleMode::Monthly,
            })
        }
    }
}

fn traffic_cycle_for_user(user: &User, now: DateTime<Utc>) -> Option<TrafficCycleContext> {
    match user.quota_reset {
        UserQuotaReset::Unlimited { .. } => {
            let start = (now.date_naive() - chrono::Days::new(29))
                .and_hms_opt(0, 0, 0)?
                .and_utc();
            let end = (now.date_naive() + chrono::Days::new(1))
                .and_hms_opt(0, 0, 0)?
                .and_utc();
            Some(TrafficCycleContext {
                start_at: rfc3339(start),
                end_at: rfc3339(end),
                mode: TrafficCycleMode::Unlimited,
            })
        }
        UserQuotaReset::Monthly {
            day_of_month,
            tz_offset_minutes,
        } => {
            let tz = CycleTimeZone::FixedOffsetMinutes { tz_offset_minutes };
            let (start, end) = current_cycle_window_at(tz, day_of_month, now).ok()?;
            Some(TrafficCycleContext {
                start_at: start.with_timezone(&Utc).to_rfc3339(),
                end_at: end.with_timezone(&Utc).to_rfc3339(),
                mode: TrafficCycleMode::Monthly,
            })
        }
    }
}

async fn sync_remote_node_histories(
    client: &MeshAwareHttpClient,
    auth: &remote::RemoteSyncAuth<'_>,
    history: &NodeHistoryHandle,
    local_node_id: &str,
    nodes: Vec<Node>,
    endpoints: Vec<crate::domain::Endpoint>,
) {
    for node in nodes {
        if node.node_id == local_node_id {
            continue;
        }
        let now = Utc::now();
        if node.api_base_url.trim().is_empty() {
            history
                .mark_sync_error(now, &node.node_id, "node api_base_url is empty".to_string())
                .await;
            continue;
        }
        let peer = peer_target_from_node(&node, &endpoints);

        for target_node_id in history.pending_node_history_cleanup(&node.node_id).await {
            match remote::clear_node_history(client, auth, &peer, &target_node_id).await {
                Ok(()) => {
                    history
                        .complete_node_history_cleanup(&node.node_id, &target_node_id)
                        .await;
                }
                Err(err) => {
                    warn!(
                        destination_node_id = %node.node_id,
                        target_node_id,
                        error = %err,
                        "retry remote node history cleanup failed"
                    );
                }
            }
        }

        for user_id in history.pending_user_traffic_cleanup(&node.node_id).await {
            match remote::clear_user_traffic(client, auth, &peer, &user_id).await {
                Ok(()) => {
                    history
                        .complete_user_traffic_cleanup(&node.node_id, &user_id)
                        .await;
                }
                Err(err) => {
                    warn!(
                        node_id = %node.node_id,
                        user_id,
                        error = %err,
                        "retry remote user traffic history cleanup failed"
                    );
                }
            }
        }

        match remote::fetch_snapshot(client, auth, &peer).await {
            Ok(snapshot) => {
                history
                    .replace_node_snapshot(now, &node.node_id, snapshot)
                    .await;
            }
            Err(err) => {
                history
                    .mark_sync_error(now, &node.node_id, err.to_string())
                    .await;
            }
        }
    }
}

fn load_history_cache(path: &Path) -> Option<PersistedNodeHistoryCache> {
    let bytes = fs::read(path).ok()?;
    let mut cache: PersistedNodeHistoryCache = serde_json::from_slice(&bytes).ok()?;
    if cache.schema_version == 1 {
        cache.schema_version = HISTORY_SCHEMA_VERSION;
    } else if cache.schema_version != HISTORY_SCHEMA_VERSION {
        return None;
    }
    Some(cache)
}

fn persist_history_cache(path: &Path, cache: &PersistedNodeHistoryCache) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(cache)?;
    write_atomic(path, &bytes)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp_path = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp_path)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(tmp_path, path)?;
    Ok(())
}

fn rfc3339(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn date_key(at: DateTime<Utc>) -> String {
    at.date_naive().format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node_runtime::{
        ComponentRuntimeStatus, NodeRuntimeEvent, NodeRuntimeSummary, RuntimeSummaryStatus,
    };

    fn component(component: RuntimeComponent, status: RuntimeStatus) -> ComponentRuntimeStatus {
        ComponentRuntimeStatus {
            component,
            status,
            last_ok_at: xp_test_fixtures::none(),
            last_fail_at: xp_test_fixtures::none(),
            down_since: xp_test_fixtures::none(),
            consecutive_failures: 0,
            recoveries_observed: 0,
            restart_attempts: 0,
            last_restart_at: xp_test_fixtures::none(),
            last_restart_fail_at: xp_test_fixtures::none(),
            last_sync_at: None,
            current_ipv4: None,
            current_ipv6: None,
            fast_mode_until: None,
            last_error: None,
        }
    }

    fn runtime(events: Vec<NodeRuntimeEvent>) -> LocalNodeRuntimeSnapshot {
        LocalNodeRuntimeSnapshot {
            node_id: xp_test_fixtures::label_node_a().to_owned(),
            summary: NodeRuntimeSummary {
                status: RuntimeSummaryStatus::Up,
                updated_at: xp_test_fixtures::timestamp_at20260520_t000000_z().to_owned(),
            },
            components: vec![
                component(RuntimeComponent::Xp, RuntimeStatus::Up),
                component(RuntimeComponent::Xray, RuntimeStatus::Down),
            ],
            recent_slots: Vec::new(),
            events,
        }
    }

    fn traffic(membership_key: &str, uplink_total: u64, downlink_total: u64) -> NodeTrafficTotals {
        NodeTrafficTotals {
            membership_key: membership_key.to_string(),
            user_id: None,
            is_probe: false,
            uplink_total,
            downlink_total,
        }
    }

    fn user_traffic(
        membership_key: &str,
        user_id: &str,
        uplink_total: u64,
        downlink_total: u64,
    ) -> NodeTrafficTotals {
        NodeTrafficTotals {
            membership_key: membership_key.to_string(),
            user_id: Some(user_id.to_string()),
            is_probe: false,
            uplink_total,
            downlink_total,
        }
    }

    fn report_with_current_points(
        window_end_at: &str,
        current: Vec<TrafficSeriesPoint>,
    ) -> TrafficReport {
        TrafficReport {
            window: "24h".to_string(),
            window_start_at: xp_test_fixtures::timestamp_at20260519_t000000_z().to_owned(),
            window_end_at: window_end_at.to_string(),
            timezone: "UTC".to_string(),
            summary: TrafficSummary {
                mode: "rolling_30d".to_string(),
                cycle_start_at: xp_test_fixtures::none(),
                cycle_end_at: xp_test_fixtures::none(),
                uplink_bytes: xp_test_fixtures::number_value0(),
                downlink_bytes: xp_test_fixtures::number_value0(),
                total_bytes: xp_test_fixtures::number_value0(),
                complete: true,
                tracking_since: None,
            },
            current,
            reference: None,
            partial: false,
            last_sample_at: Some(xp_test_fixtures::timestamp_at20260520_t000000_z().to_owned()),
            warnings: Vec::new(),
        }
    }

    #[tokio::test]
    async fn records_daily_traffic_delta_and_component_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let t0 = "2026-05-20T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let t1 = "2026-05-20T00:05:00Z".parse::<DateTime<Utc>>().unwrap();

        handle
            .record_local_sample(
                t0,
                "node-a",
                Some(vec![traffic("membership-a", 100, 300)]),
                runtime(Vec::new()),
            )
            .await;
        handle
            .record_local_sample(
                t1,
                "node-a",
                Some(vec![traffic("membership-a", 160, 380)]),
                runtime(Vec::new()),
            )
            .await;

        let snapshot = handle.snapshot("node-a").await.unwrap();
        assert_eq!(snapshot.daily_traffic.len(), 1);
        assert_eq!(snapshot.daily_traffic[0].uplink_bytes, 60);
        assert_eq!(snapshot.daily_traffic[0].downlink_bytes, 80);
        assert_eq!(snapshot.daily_component_status.len(), 1);
        assert_eq!(snapshot.daily_component_status[0].components.len(), 2);
    }

    #[tokio::test]
    async fn midnight_delta_is_attributed_to_the_closing_utc_day() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let before_midnight = "2026-05-19T23:55:00Z".parse::<DateTime<Utc>>().unwrap();
        let midnight = "2026-05-20T00:00:00Z".parse::<DateTime<Utc>>().unwrap();

        handle
            .record_local_sample(
                before_midnight,
                "node-a",
                Some(vec![traffic("membership-a", 100, 300)]),
                runtime(Vec::new()),
            )
            .await;
        handle
            .record_local_sample(
                midnight,
                "node-a",
                Some(vec![traffic("membership-a", 160, 380)]),
                runtime(Vec::new()),
            )
            .await;

        let snapshot = handle.snapshot("node-a").await.unwrap();
        assert_eq!(snapshot.daily_traffic.len(), 1);
        assert_eq!(snapshot.daily_traffic[0].date, "2026-05-19");
        assert_eq!(snapshot.daily_traffic[0].uplink_bytes, 60);
        assert_eq!(snapshot.daily_traffic[0].downlink_bytes, 80);
    }

    #[tokio::test]
    async fn empty_membership_sample_is_known_zero_traffic() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let now = "2026-05-20T00:05:00Z".parse::<DateTime<Utc>>().unwrap();

        handle
            .record_local_sample(now, "node-a", Some(Vec::new()), runtime(Vec::new()))
            .await;

        let rollup = handle.snapshot("node-a").await.unwrap().traffic.unwrap();
        let bucket = rollup.five_minute.last().unwrap();
        assert_eq!(bucket.uplink_bytes, Some(0));
        assert_eq!(bucket.downlink_bytes, Some(0));
        assert!(bucket.complete);
    }

    #[tokio::test]
    async fn counter_reset_updates_baseline_without_negative_delta() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let t0 = "2026-05-20T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let t1 = "2026-05-20T00:05:00Z".parse::<DateTime<Utc>>().unwrap();
        let t2 = "2026-05-20T00:10:00Z".parse::<DateTime<Utc>>().unwrap();

        for (at, up, down) in [(t0, 100, 300), (t1, 90, 290), (t2, 120, 340)] {
            handle
                .record_local_sample(
                    at,
                    "node-a",
                    Some(vec![traffic("membership-a", up, down)]),
                    runtime(Vec::new()),
                )
                .await;
        }

        let snapshot = handle.snapshot("node-a").await.unwrap();
        assert_eq!(snapshot.daily_traffic[0].uplink_bytes, 30);
        assert_eq!(snapshot.daily_traffic[0].downlink_bytes, 50);
    }

    #[tokio::test]
    async fn records_five_minute_node_and_user_rollups_from_one_delta() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let t0 = "2026-05-20T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let t1 = "2026-05-20T00:05:00Z".parse::<DateTime<Utc>>().unwrap();

        for (at, up, down) in [(t0, 100, 300), (t1, 160, 380)] {
            handle
                .record_local_sample(
                    at,
                    "node-a",
                    Some(vec![user_traffic("membership-a", "user-a", up, down)]),
                    runtime(Vec::new()),
                )
                .await;
        }

        let snapshot = handle.snapshot("node-a").await.unwrap();
        let rollup = snapshot.traffic.unwrap();
        assert_eq!(rollup.five_minute.len(), 2);
        assert_eq!(rollup.five_minute[1].uplink_bytes, Some(60));
        assert_eq!(rollup.five_minute[1].downlink_bytes, Some(80));
        let report = handle
            .user_traffic_report("user-a", Some("node-a"), TrafficWindow::Hours24, t1)
            .await
            .unwrap();
        assert_eq!(report.current.last().unwrap().uplink_bytes, Some(60));
        assert_eq!(report.current.last().unwrap().downlink_bytes, Some(80));
    }

    #[tokio::test]
    async fn first_traffic_sample_is_partial_and_explains_tracking_start() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let now = "2026-05-20T00:05:00Z".parse::<DateTime<Utc>>().unwrap();

        handle
            .record_local_sample(
                now,
                "node-a",
                Some(vec![traffic("membership-a", 100, 300)]),
                runtime(Vec::new()),
            )
            .await;

        let snapshot = handle.snapshot("node-a").await.unwrap();
        let bucket = snapshot.traffic.unwrap().five_minute.pop().unwrap();
        assert!(!bucket.complete);
        assert!(bucket.uplink_bytes.is_none());
        assert!(
            bucket
                .warnings
                .iter()
                .any(|warning| warning.contains("tracking started"))
        );
    }

    #[tokio::test]
    async fn cycle_tracking_since_uses_first_sample_time() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let sampled_at = "2026-05-20T00:05:00Z".parse::<DateTime<Utc>>().unwrap();
        handle
            .record_local_sample_with_status(
                sampled_at,
                "node-a",
                Some(NodeTrafficSample {
                    totals: vec![traffic("membership-a", 100, 300)],
                    unavailable_users: BTreeSet::new(),
                    complete: true,
                    warnings: Vec::new(),
                    cycle: Some(TrafficCycleContext {
                        start_at: xp_test_fixtures::timestamp_at20260501_t000000_z().to_owned(),
                        end_at: xp_test_fixtures::timestamp_at20260601_t000000_z().to_owned(),
                        mode: TrafficCycleMode::Monthly,
                    }),
                    user_cycles: BTreeMap::new(),
                }),
                runtime(Vec::new()),
            )
            .await;

        let cycle = handle
            .snapshot("node-a")
            .await
            .unwrap()
            .traffic
            .unwrap()
            .cycle
            .unwrap();
        assert_eq!(cycle.tracking_since, "2026-05-20T00:05:00Z");
        assert!(!cycle.complete);
    }

    #[tokio::test]
    async fn unavailable_user_gets_a_nullable_partial_bucket() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let now = "2026-05-20T00:05:00Z".parse::<DateTime<Utc>>().unwrap();

        handle
            .record_local_sample_with_status(
                now,
                "node-a",
                Some(NodeTrafficSample {
                    totals: Vec::new(),
                    unavailable_users: BTreeSet::from(["user-a".to_string()]),
                    complete: false,
                    warnings: vec!["membership sample unavailable".to_string()],
                    cycle: None,
                    user_cycles: BTreeMap::new(),
                }),
                runtime(Vec::new()),
            )
            .await;

        let report = handle
            .user_traffic_report("user-a", Some("node-a"), TrafficWindow::Hours24, now)
            .await
            .unwrap();
        assert!(report.partial);
        assert!(
            report
                .current
                .iter()
                .any(|point| point.total_bytes.is_none())
        );
    }

    #[tokio::test]
    async fn user_sampling_failure_does_not_make_other_users_partial() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let first = "2026-05-20T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let second = "2026-05-20T00:05:00Z".parse::<DateTime<Utc>>().unwrap();

        handle
            .record_local_sample(
                first,
                "node-a",
                Some(vec![
                    user_traffic("membership-a", "user-a", 100, 300),
                    user_traffic("membership-b", "user-b", 200, 500),
                ]),
                runtime(Vec::new()),
            )
            .await;
        handle
            .record_local_sample_with_status(
                second,
                "node-a",
                Some(NodeTrafficSample {
                    totals: vec![user_traffic("membership-b", "user-b", 220, 560)],
                    unavailable_users: BTreeSet::from(["user-a".to_string()]),
                    complete: false,
                    warnings: vec!["user-a sample unavailable".to_string()],
                    cycle: None,
                    user_cycles: BTreeMap::new(),
                }),
                runtime(Vec::new()),
            )
            .await;

        let user_a = handle
            .user_traffic_report("user-a", Some("node-a"), TrafficWindow::Hours24, second)
            .await
            .unwrap();
        let user_b = handle
            .user_traffic_report("user-b", Some("node-a"), TrafficWindow::Hours24, second)
            .await
            .unwrap();
        let node = handle
            .node_traffic_report("node-a", TrafficWindow::Hours24, second)
            .await
            .unwrap();
        assert!(user_a.partial);
        assert!(user_a.current.last().unwrap().total_bytes.is_none());
        assert!(user_b.current.last().unwrap().complete);
        assert_eq!(user_b.current.last().unwrap().uplink_bytes, Some(20));
        assert_eq!(user_b.current.last().unwrap().downlink_bytes, Some(60));
        assert!(node.partial);
        assert!(node.current.last().unwrap().total_bytes.is_none());
    }

    #[test]
    fn cycle_accumulator_can_be_complete_after_a_valid_starting_bucket() {
        let context = TrafficCycleContext {
            start_at: xp_test_fixtures::timestamp_at20260501_t000000_z().to_owned(),
            end_at: xp_test_fixtures::timestamp_at20260601_t000000_z().to_owned(),
            mode: TrafficCycleMode::Monthly,
        };
        let mut accumulator = Some(TrafficCycleAccumulator {
            mode: "monthly".to_string(),
            start_at: xp_test_fixtures::timestamp_at20260401_t000000_z().to_owned(),
            end_at: xp_test_fixtures::timestamp_at20260501_t000000_z().to_owned(),
            uplink_bytes: xp_test_fixtures::number_value100(),
            downlink_bytes: xp_test_fixtures::number_value200(),
            complete: true,
            tracking_since: "2026-04-01T00:05:00Z".to_string(),
            warnings: Vec::new(),
        });
        update_cycle_accumulator(
            &mut accumulator,
            Some(&context),
            Some(10),
            Some(20),
            true,
            &[],
            "2026-05-20T00:05:00Z",
        );
        assert!(accumulator.unwrap().complete);
    }

    #[test]
    fn cycle_configuration_warning_is_included_in_report_warnings() {
        let first = TrafficCycleContext {
            start_at: xp_test_fixtures::timestamp_at20260501_t000000_z().to_owned(),
            end_at: xp_test_fixtures::timestamp_at20260615_t000000_z().to_owned(),
            mode: TrafficCycleMode::Monthly,
        };
        let second = TrafficCycleContext {
            start_at: xp_test_fixtures::timestamp_at20260601_t000000_z().to_owned(),
            end_at: xp_test_fixtures::timestamp_at20260701_t000000_z().to_owned(),
            mode: TrafficCycleMode::Monthly,
        };
        let mut accumulator = None;
        update_cycle_accumulator(
            &mut accumulator,
            Some(&first),
            Some(10),
            Some(20),
            true,
            &[],
            "2026-05-20T00:05:00Z",
        );
        update_cycle_accumulator(
            &mut accumulator,
            Some(&second),
            Some(1),
            Some(2),
            true,
            &[],
            "2026-06-01T00:05:00Z",
        );
        let rollup = NodeTrafficRollupSnapshot {
            cycle: accumulator,
            ..NodeTrafficRollupSnapshot::default()
        };
        let warnings = summary_warnings(&rollup, &[], &[], TrafficWindow::Hours24);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("configuration changed"))
        );
    }

    #[test]
    fn summary_warnings_only_include_buckets_in_requested_window() {
        let rollup = NodeTrafficRollupSnapshot {
            five_minute: vec![
                NodeTrafficBucket {
                    start_at: "2026-07-28T00:00:00Z".to_string(),
                    end_at: "2026-07-28T00:05:00Z".to_string(),
                    uplink_bytes: None,
                    downlink_bytes: None,
                    complete: false,
                    warnings: vec!["old five-minute warning".to_string()],
                },
                NodeTrafficBucket {
                    start_at: "2026-07-29T00:00:00Z".to_string(),
                    end_at: "2026-07-29T00:05:00Z".to_string(),
                    uplink_bytes: None,
                    downlink_bytes: None,
                    complete: false,
                    warnings: vec!["current five-minute warning".to_string()],
                },
            ],
            daily: vec![NodeTrafficDailyBucket {
                date: "2026-07-28".to_string(),
                uplink_bytes: None,
                downlink_bytes: None,
                complete: false,
                warnings: vec!["daily warning".to_string()],
            }],
            ..NodeTrafficRollupSnapshot::default()
        };
        let current = vec![TrafficSeriesPoint {
            start_at: "2026-07-29T00:00:00Z".to_string(),
            end_at: "2026-07-29T00:05:00Z".to_string(),
            uplink_bytes: None,
            downlink_bytes: None,
            total_bytes: None,
            complete: false,
            is_current_day: false,
        }];
        let warnings = summary_warnings(&rollup, &current, &[], TrafficWindow::Hours24);
        assert!(
            warnings
                .iter()
                .any(|warning| warning == "current five-minute warning")
        );
        assert!(
            !warnings
                .iter()
                .any(|warning| warning == "old five-minute warning")
        );

        let daily_current = vec![TrafficSeriesPoint {
            start_at: "2026-07-28T00:00:00Z".to_string(),
            end_at: "2026-07-29T00:00:00Z".to_string(),
            uplink_bytes: None,
            downlink_bytes: None,
            total_bytes: None,
            complete: false,
            is_current_day: false,
        }];
        let warnings = summary_warnings(&rollup, &daily_current, &[], TrafficWindow::Days31);
        assert!(warnings.iter().any(|warning| warning == "daily warning"));
        assert!(
            !warnings
                .iter()
                .any(|warning| warning == "old five-minute warning")
        );
    }

    #[test]
    fn inactive_user_records_do_not_create_future_gaps_or_replace_active_cycle() {
        let inactive = PersistedUserTrafficRecord {
            five_minute: vec![NodeTrafficBucket {
                start_at: "2026-07-28T00:00:00Z".to_string(),
                end_at: "2026-07-28T00:05:00Z".to_string(),
                uplink_bytes: Some(10),
                downlink_bytes: Some(20),
                complete: true,
                warnings: Vec::new(),
            }],
            cycle: Some(TrafficCycleAccumulator {
                mode: "monthly".to_string(),
                start_at: xp_test_fixtures::timestamp_at20260701_t000000_z().to_owned(),
                end_at: xp_test_fixtures::timestamp_at20260801_t000000_z().to_owned(),
                uplink_bytes: xp_test_fixtures::number_value10(),
                downlink_bytes: xp_test_fixtures::number_value20(),
                complete: true,
                tracking_since: "2026-07-28T00:05:00Z".to_string(),
                warnings: Vec::new(),
            }),
            membership_active: false,
            ..PersistedUserTrafficRecord::default()
        };
        let active = PersistedUserTrafficRecord {
            five_minute: vec![NodeTrafficBucket {
                start_at: "2026-07-29T00:00:00Z".to_string(),
                end_at: "2026-07-29T00:05:00Z".to_string(),
                uplink_bytes: Some(30),
                downlink_bytes: Some(40),
                complete: true,
                warnings: Vec::new(),
            }],
            cycle: Some(TrafficCycleAccumulator {
                mode: "monthly".to_string(),
                start_at: xp_test_fixtures::timestamp_at20260801_t000000_z().to_owned(),
                end_at: xp_test_fixtures::timestamp_at20260901_t000000_z().to_owned(),
                uplink_bytes: xp_test_fixtures::number_value30(),
                downlink_bytes: xp_test_fixtures::number_value40(),
                complete: true,
                tracking_since: "2026-08-01T00:05:00Z".to_string(),
                warnings: Vec::new(),
            }),
            membership_active: true,
            ..PersistedUserTrafficRecord::default()
        };

        let aggregate = aggregate_user_records(&[&inactive, &active]);
        assert!(aggregate.five_minute.iter().all(|bucket| bucket.complete));
        assert_eq!(aggregate.cycle.unwrap().start_at, "2026-08-01T00:00:00Z");
    }

    #[tokio::test]
    async fn user_traffic_cleanup_tombstone_survives_reload_until_completed() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("node_history_cache.json");
        let handle = NodeHistoryHandle::new(path.clone());
        handle.queue_user_traffic_cleanup("node-a", "user-a").await;
        handle.queue_node_history_cleanup("node-b", "node-a").await;
        assert_eq!(
            handle.pending_user_traffic_cleanup("node-a").await,
            vec!["user-a"]
        );
        assert_eq!(
            handle.pending_node_history_cleanup("node-b").await,
            vec!["node-a"]
        );

        let reloaded = NodeHistoryHandle::new(path);
        assert_eq!(
            reloaded.pending_user_traffic_cleanup("node-a").await,
            vec!["user-a"]
        );
        assert_eq!(
            reloaded.pending_node_history_cleanup("node-b").await,
            vec!["node-a"]
        );
        reloaded
            .complete_user_traffic_cleanup("node-a", "user-a")
            .await;
        reloaded
            .complete_node_history_cleanup("node-b", "node-a")
            .await;
        assert!(
            reloaded
                .pending_user_traffic_cleanup("node-a")
                .await
                .is_empty()
        );
        assert!(
            reloaded
                .pending_node_history_cleanup("node-b")
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn user_traffic_node_ids_include_retained_history_only() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let now = "2026-07-29T00:05:00Z".parse::<DateTime<Utc>>().unwrap();
        handle
            .record_local_sample(
                now,
                "node-a",
                Some(vec![user_traffic("membership-a", "user-a", 100, 200)]),
                runtime(Vec::new()),
            )
            .await;
        handle
            .record_local_sample(
                now,
                "node-b",
                Some(vec![user_traffic("membership-b", "user-b", 100, 200)]),
                runtime(Vec::new()),
            )
            .await;

        assert_eq!(
            handle.user_traffic_node_ids("user-a").await,
            BTreeSet::from(["node-a".to_string()])
        );
    }

    #[tokio::test]
    async fn mirrored_node_snapshot_keeps_remote_user_history_index() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let now = "2026-07-29T00:05:00Z".parse::<DateTime<Utc>>().unwrap();

        handle
            .replace_node_snapshot(
                now,
                "node-b",
                NodeHistorySnapshot {
                    node_id: xp_test_fixtures::label_node_b().to_owned(),
                    last_synced_at: Some(
                        xp_test_fixtures::timestamp_at20260729_t000500_z().to_owned(),
                    ),
                    last_sync_error: None,
                    daily_traffic: Vec::new(),
                    daily_component_status: Vec::new(),
                    component_status_events: Vec::new(),
                    traffic: Some(NodeTrafficRollupSnapshot::default()),
                    user_traffic_users: vec!["user-a".to_string()],
                },
            )
            .await;

        assert_eq!(
            handle.user_traffic_node_ids("user-a").await,
            BTreeSet::from(["node-b".to_string()])
        );
    }

    #[tokio::test]
    async fn retained_user_cycle_resets_without_active_membership() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let first = "2026-05-31T23:55:00Z".parse::<DateTime<Utc>>().unwrap();
        let second = "2026-06-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let old_cycle = TrafficCycleContext {
            start_at: xp_test_fixtures::timestamp_at20260501_t000000_z().to_owned(),
            end_at: xp_test_fixtures::timestamp_at20260601_t000000_z().to_owned(),
            mode: TrafficCycleMode::Monthly,
        };
        let new_cycle = TrafficCycleContext {
            start_at: xp_test_fixtures::timestamp_at20260601_t000000_z().to_owned(),
            end_at: xp_test_fixtures::timestamp_at20260701_t000000_z().to_owned(),
            mode: TrafficCycleMode::Monthly,
        };

        handle
            .record_local_sample_with_status(
                first,
                "node-a",
                Some(NodeTrafficSample {
                    totals: vec![user_traffic("membership-a", "user-a", 100, 200)],
                    unavailable_users: BTreeSet::new(),
                    complete: true,
                    warnings: Vec::new(),
                    cycle: None,
                    user_cycles: BTreeMap::from([("user-a".to_string(), old_cycle)]),
                }),
                runtime(Vec::new()),
            )
            .await;
        handle
            .record_local_sample_with_status(
                second,
                "node-a",
                Some(NodeTrafficSample {
                    totals: Vec::new(),
                    unavailable_users: BTreeSet::new(),
                    complete: true,
                    warnings: Vec::new(),
                    cycle: None,
                    user_cycles: BTreeMap::from([("user-a".to_string(), new_cycle.clone())]),
                }),
                runtime(Vec::new()),
            )
            .await;
        let third = "2026-06-01T00:05:00Z".parse::<DateTime<Utc>>().unwrap();
        handle
            .record_local_sample_with_status(
                third,
                "node-a",
                Some(NodeTrafficSample {
                    totals: Vec::new(),
                    unavailable_users: BTreeSet::new(),
                    complete: true,
                    warnings: Vec::new(),
                    cycle: None,
                    user_cycles: BTreeMap::from([("user-a".to_string(), new_cycle)]),
                }),
                runtime(Vec::new()),
            )
            .await;

        let state = handle.snapshot("node-a").await.unwrap();
        let record = handle
            .user_traffic_report("user-a", Some("node-a"), TrafficWindow::Hours24, third)
            .await
            .unwrap();
        assert_eq!(
            record.summary.cycle_start_at.as_deref(),
            Some("2026-06-01T00:00:00Z")
        );
        assert_eq!(record.summary.uplink_bytes, 0);
        assert_eq!(record.current.last().unwrap().total_bytes, None);
        assert!(!record.current.last().unwrap().complete);
        assert!(state.traffic.unwrap().five_minute.len() >= 2);
    }

    #[tokio::test]
    async fn daily_rollup_uses_five_minute_bucket_utc_date() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let first = "2026-05-19T23:55:00Z".parse::<DateTime<Utc>>().unwrap();
        let boundary = "2026-05-20T00:00:00Z".parse::<DateTime<Utc>>().unwrap();

        for (at, up, down) in [(first, 100, 300), (boundary, 160, 380)] {
            handle
                .record_local_sample(
                    at,
                    "node-a",
                    Some(vec![traffic("membership-a", up, down)]),
                    runtime(Vec::new()),
                )
                .await;
        }

        let snapshot = handle.snapshot("node-a").await.unwrap();
        let daily = snapshot.traffic.unwrap().daily;
        assert_eq!(daily.len(), 1);
        assert_eq!(daily[0].date, "2026-05-19");
        assert!(!daily[0].complete);
        assert_eq!(daily[0].uplink_bytes, None);
        assert_eq!(daily[0].downlink_bytes, None);
    }

    #[tokio::test]
    async fn sampling_gap_across_utc_midnight_marks_both_daily_buckets_partial() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let first = "2026-05-19T23:50:00Z".parse::<DateTime<Utc>>().unwrap();
        let recovered = "2026-05-20T00:05:00Z".parse::<DateTime<Utc>>().unwrap();
        for (at, up, down) in [(first, 100, 300), (recovered, 160, 380)] {
            handle
                .record_local_sample(
                    at,
                    "node-a",
                    Some(vec![traffic("membership-a", up, down)]),
                    runtime(Vec::new()),
                )
                .await;
        }

        let daily = handle
            .snapshot("node-a")
            .await
            .unwrap()
            .traffic
            .unwrap()
            .daily;
        assert!(
            daily
                .iter()
                .any(|bucket| { bucket.date == "2026-05-19" && !bucket.complete })
        );
        assert!(
            daily
                .iter()
                .any(|bucket| { bucket.date == "2026-05-20" && !bucket.complete })
        );
    }

    #[tokio::test]
    async fn incomplete_daily_bucket_clears_prior_values() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let first = "2026-05-20T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let second = "2026-05-20T00:05:00Z".parse::<DateTime<Utc>>().unwrap();
        let third = "2026-05-20T00:10:00Z".parse::<DateTime<Utc>>().unwrap();

        handle
            .record_local_sample(
                first,
                "node-a",
                Some(vec![user_traffic("membership-a", "user-a", 100, 200)]),
                runtime(Vec::new()),
            )
            .await;
        handle
            .record_local_sample(
                second,
                "node-a",
                Some(vec![user_traffic("membership-a", "user-a", 120, 240)]),
                runtime(Vec::new()),
            )
            .await;
        handle
            .record_local_sample_with_status(
                third,
                "node-a",
                Some(NodeTrafficSample {
                    totals: Vec::new(),
                    unavailable_users: BTreeSet::from(["user-a".to_string()]),
                    complete: false,
                    warnings: vec!["user-a sample unavailable".to_string()],
                    cycle: None,
                    user_cycles: BTreeMap::new(),
                }),
                runtime(Vec::new()),
            )
            .await;

        let bucket = handle
            .snapshot("node-a")
            .await
            .unwrap()
            .traffic
            .unwrap()
            .daily
            .into_iter()
            .find(|bucket| bucket.date == "2026-05-20")
            .unwrap();
        assert!(!bucket.complete);
        assert_eq!(bucket.uplink_bytes, None);
        assert_eq!(bucket.downlink_bytes, None);
    }

    #[test]
    fn rolling_summary_is_partial_when_an_expected_utc_day_is_missing() {
        let latest = "2026-05-20T12:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let summary = build_summary(
            &NodeTrafficRollupSnapshot {
                daily: vec![NodeTrafficDailyBucket {
                    date: "2026-05-20".to_string(),
                    uplink_bytes: Some(10),
                    downlink_bytes: Some(20),
                    complete: true,
                    warnings: Vec::new(),
                }],
                ..NodeTrafficRollupSnapshot::default()
            },
            latest,
        );
        assert!(!summary.complete);
        assert_eq!(summary.uplink_bytes, 10);
        assert_eq!(summary.downlink_bytes, 20);
        assert_eq!(
            summary.tracking_since.as_deref(),
            Some("2026-05-20T00:00:00Z")
        );
    }

    #[test]
    fn stale_current_day_is_nullable_and_partial() {
        let latest = "2026-05-20T12:05:00Z".parse::<DateTime<Utc>>().unwrap();
        let report = build_traffic_report(
            &NodeTrafficRollupSnapshot {
                daily: vec![NodeTrafficDailyBucket {
                    date: "2026-05-20".to_string(),
                    uplink_bytes: Some(10),
                    downlink_bytes: Some(20),
                    complete: true,
                    warnings: Vec::new(),
                }],
                cycle: Some(TrafficCycleAccumulator {
                    mode: "monthly".to_string(),
                    start_at: xp_test_fixtures::timestamp_at20260501_t000000_z().to_owned(),
                    end_at: xp_test_fixtures::timestamp_at20260601_t000000_z().to_owned(),
                    uplink_bytes: xp_test_fixtures::number_value10(),
                    downlink_bytes: xp_test_fixtures::number_value20(),
                    complete: true,
                    tracking_since: "2026-05-01T00:00:00Z".to_string(),
                    warnings: Vec::new(),
                }),
                last_sample_at: Some(xp_test_fixtures::timestamp_at20260520_t115500_z().to_owned()),
                ..NodeTrafficRollupSnapshot::default()
            },
            TrafficWindow::Days31,
            latest,
        );

        let current_day = report
            .current
            .iter()
            .find(|point| point.is_current_day)
            .unwrap();
        assert!(!current_day.complete);
        assert!(current_day.total_bytes.is_none());
        assert!(!report.summary.complete);
        assert!(report.partial);
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("stale"))
        );
    }

    #[tokio::test]
    async fn deleting_user_and_node_clears_traffic_history() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let first = "2026-05-20T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let second = "2026-05-20T00:05:00Z".parse::<DateTime<Utc>>().unwrap();
        for (at, up, down) in [(first, 100, 300), (second, 160, 380)] {
            handle
                .record_local_sample(
                    at,
                    "node-a",
                    Some(vec![user_traffic("membership-a", "user-a", up, down)]),
                    runtime(Vec::new()),
                )
                .await;
        }

        assert!(
            handle
                .user_traffic_report("user-a", Some("node-a"), TrafficWindow::Hours24, second)
                .await
                .is_some()
        );
        handle.clear_user("user-a").await;
        assert!(
            handle
                .user_traffic_report("user-a", Some("node-a"), TrafficWindow::Hours24, second)
                .await
                .is_none()
        );
        handle
            .record_local_sample(
                "2026-05-20T00:10:00Z".parse().unwrap(),
                "node-a",
                Some(vec![user_traffic("membership-a", "user-a", 200, 450)]),
                runtime(Vec::new()),
            )
            .await;
        assert!(
            handle
                .user_traffic_report("user-a", Some("node-a"), TrafficWindow::Hours24, second)
                .await
                .is_none()
        );
        assert!(
            handle
                .node_traffic_report("node-a", TrafficWindow::Hours24, second)
                .await
                .is_some()
        );
        handle.clear_node("node-a").await;
        assert!(handle.snapshot("node-a").await.is_none());
    }

    #[tokio::test]
    async fn deleted_node_tombstone_blocks_stale_snapshot_recreation() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("node_history_cache.json");
        let handle = NodeHistoryHandle::new(path.clone());
        let now = "2026-05-20T00:00:00Z".parse::<DateTime<Utc>>().unwrap();

        handle.clear_node("node-a").await;
        handle
            .replace_node_snapshot(
                now,
                "node-a",
                NodeHistorySnapshot {
                    node_id: xp_test_fixtures::label_node_a().to_owned(),
                    last_synced_at: xp_test_fixtures::none(),
                    last_sync_error: None,
                    daily_traffic: Vec::new(),
                    daily_component_status: Vec::new(),
                    component_status_events: Vec::new(),
                    traffic: Some(NodeTrafficRollupSnapshot::default()),
                    user_traffic_users: Vec::new(),
                },
            )
            .await;

        assert!(handle.snapshot("node-a").await.is_none());
        let reloaded = NodeHistoryHandle::new(path);
        assert!(reloaded.snapshot("node-a").await.is_none());
    }

    #[tokio::test]
    async fn removed_membership_history_expires_after_retention_window() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let first = "2026-05-20T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let second = "2026-05-20T00:05:00Z".parse::<DateTime<Utc>>().unwrap();
        let removed = "2026-05-20T00:10:00Z".parse::<DateTime<Utc>>().unwrap();
        let expired = "2026-08-19T00:15:00Z".parse::<DateTime<Utc>>().unwrap();
        let cycle = TrafficCycleContext {
            start_at: xp_test_fixtures::timestamp_at20260501_t000000_z().to_owned(),
            end_at: xp_test_fixtures::timestamp_at20260601_t000000_z().to_owned(),
            mode: TrafficCycleMode::Monthly,
        };
        let cycles = || BTreeMap::from([("user-a".to_string(), cycle.clone())]);

        handle
            .record_local_sample_with_status(
                first,
                "node-a",
                Some(NodeTrafficSample {
                    totals: vec![user_traffic("membership-a", "user-a", 100, 200)],
                    unavailable_users: BTreeSet::new(),
                    complete: true,
                    warnings: Vec::new(),
                    cycle: None,
                    user_cycles: cycles(),
                }),
                runtime(Vec::new()),
            )
            .await;
        handle
            .record_local_sample_with_status(
                second,
                "node-a",
                Some(NodeTrafficSample {
                    totals: vec![user_traffic("membership-a", "user-a", 130, 250)],
                    unavailable_users: BTreeSet::new(),
                    complete: true,
                    warnings: Vec::new(),
                    cycle: None,
                    user_cycles: cycles(),
                }),
                runtime(Vec::new()),
            )
            .await;
        handle
            .record_local_sample_with_status(
                removed,
                "node-a",
                Some(NodeTrafficSample {
                    totals: Vec::new(),
                    unavailable_users: BTreeSet::new(),
                    complete: true,
                    warnings: Vec::new(),
                    cycle: None,
                    user_cycles: cycles(),
                }),
                runtime(Vec::new()),
            )
            .await;
        assert!(
            handle
                .user_traffic_report("user-a", Some("node-a"), TrafficWindow::Hours24, removed)
                .await
                .is_some()
        );

        handle
            .record_local_sample_with_status(
                expired,
                "node-a",
                Some(NodeTrafficSample {
                    totals: Vec::new(),
                    unavailable_users: BTreeSet::new(),
                    complete: true,
                    warnings: Vec::new(),
                    cycle: None,
                    user_cycles: cycles(),
                }),
                runtime(Vec::new()),
            )
            .await;
        assert!(
            handle
                .user_traffic_report("user-a", Some("node-a"), TrafficWindow::Hours24, expired)
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn schema_v1_loads_with_empty_new_traffic_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("node_history_cache.json");
        fs::write(
            &path,
            r#"{
              "schema_version": 1,
              "nodes": {
                "node-a": {
                  "node_id": "node-a",
                  "daily_traffic": {},
                  "daily_component_status": {},
                  "component_status_events": []
                }
              }
            }"#,
        )
        .unwrap();

        let handle = NodeHistoryHandle::new(path);
        let snapshot = handle.snapshot("node-a").await.unwrap();
        let traffic = snapshot.traffic.unwrap();
        assert!(traffic.five_minute.is_empty());
        assert!(traffic.daily.is_empty());
        assert!(traffic.cycle.is_none());
    }

    #[tokio::test]
    async fn missing_five_minute_boundary_is_a_null_point() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let t0 = "2026-05-20T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let t1 = "2026-05-20T00:05:00Z".parse::<DateTime<Utc>>().unwrap();
        let t2 = "2026-05-20T00:15:00Z".parse::<DateTime<Utc>>().unwrap();
        for (at, up, down) in [(t0, 100, 300), (t1, 160, 380), (t2, 220, 460)] {
            handle
                .record_local_sample(
                    at,
                    "node-a",
                    Some(vec![traffic("membership-a", up, down)]),
                    runtime(Vec::new()),
                )
                .await;
        }
        let report = handle
            .node_traffic_report("node-a", TrafficWindow::Hours24, t2)
            .await
            .unwrap();
        assert!(report.partial);
        assert!(
            report
                .current
                .iter()
                .any(|point| point.uplink_bytes.is_none())
        );
    }

    #[test]
    fn merged_reports_align_by_timestamp_and_preserve_missing_node_values() {
        let first = report_with_current_points(
            "2026-05-20T00:10:00Z",
            vec![
                traffic_point(
                    "2026-05-20T00:00:00Z".to_string(),
                    "2026-05-20T00:05:00Z".to_string(),
                    Some(100),
                    Some(1),
                    true,
                    false,
                ),
                traffic_point(
                    "2026-05-20T00:05:00Z".to_string(),
                    "2026-05-20T00:10:00Z".to_string(),
                    Some(10),
                    Some(1),
                    true,
                    false,
                ),
            ],
        );
        let second = report_with_current_points(
            "2026-05-20T00:15:00Z",
            vec![
                traffic_point(
                    "2026-05-20T00:05:00Z".to_string(),
                    "2026-05-20T00:10:00Z".to_string(),
                    Some(20),
                    Some(2),
                    true,
                    false,
                ),
                traffic_point(
                    "2026-05-20T00:10:00Z".to_string(),
                    "2026-05-20T00:15:00Z".to_string(),
                    Some(30),
                    Some(3),
                    true,
                    false,
                ),
            ],
        );

        let merged = merge_traffic_reports(
            &[first, second],
            TrafficWindow::Hours24,
            "2026-05-20T00:15:00Z".parse().unwrap(),
        );
        let at_five = merged
            .current
            .iter()
            .find(|point| point.start_at == "2026-05-20T00:05:00Z")
            .unwrap();
        let at_ten = merged
            .current
            .iter()
            .find(|point| point.start_at == "2026-05-20T00:10:00Z")
            .unwrap();
        assert_eq!(at_five.uplink_bytes, Some(30));
        assert_eq!(at_ten.uplink_bytes, None);
        assert!(merged.partial);
    }

    #[test]
    fn merged_reports_do_not_sum_incompatible_quota_cycles() {
        let mut first = report_with_current_points("2026-05-20T00:10:00Z", Vec::new());
        first.summary.mode = "cycle".to_string();
        first.summary.cycle_start_at =
            Some(xp_test_fixtures::timestamp_at20260501_t000000_z().to_owned());
        first.summary.cycle_end_at =
            Some(xp_test_fixtures::timestamp_at20260601_t000000_z().to_owned());
        first.summary.uplink_bytes = xp_test_fixtures::number_value100();
        first.summary.downlink_bytes = xp_test_fixtures::number_value200();
        first.summary.total_bytes = xp_test_fixtures::number_value300();

        let mut second = report_with_current_points("2026-05-20T00:10:00Z", Vec::new());
        second.summary.mode = "cycle".to_string();
        second.summary.cycle_start_at =
            Some(xp_test_fixtures::timestamp_at20260515_t000000_z().to_owned());
        second.summary.cycle_end_at =
            Some(xp_test_fixtures::timestamp_at20260615_t000000_z().to_owned());
        second.summary.uplink_bytes = xp_test_fixtures::number_value40();
        second.summary.downlink_bytes = xp_test_fixtures::number_value50();
        second.summary.total_bytes = xp_test_fixtures::number_value900();

        let merged = merge_traffic_reports(
            &[first, second],
            TrafficWindow::Hours24,
            "2026-05-20T00:10:00Z".parse().unwrap(),
        );

        assert_eq!(merged.summary.uplink_bytes, 0);
        assert_eq!(merged.summary.downlink_bytes, 0);
        assert_eq!(merged.summary.total_bytes, 0);
        assert!(!merged.summary.complete);
        assert!(merged.summary.cycle_start_at.is_none());
        assert!(
            merged
                .warnings
                .iter()
                .any(|warning| warning == "traffic summaries span different quota cycles")
        );
    }

    #[tokio::test]
    async fn five_minute_rollup_is_capped_at_588_buckets() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let base = "2026-05-20T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        for index in 0..=600 {
            let at = base + ChronoDuration::minutes(index * 5);
            handle
                .record_local_sample(
                    at,
                    "node-a",
                    Some(vec![traffic("membership-a", index as u64, index as u64)]),
                    runtime(Vec::new()),
                )
                .await;
        }
        let snapshot = handle.snapshot("node-a").await.unwrap();
        assert!(snapshot.traffic.unwrap().five_minute.len() <= TRAFFIC_ROLLUP_BUCKETS);
    }

    #[tokio::test]
    async fn missing_membership_sample_does_not_advance_its_baseline() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let t0 = "2026-05-20T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let t1 = "2026-05-20T00:05:00Z".parse::<DateTime<Utc>>().unwrap();
        let t2 = "2026-05-20T00:10:00Z".parse::<DateTime<Utc>>().unwrap();

        handle
            .record_local_sample(
                t0,
                "node-a",
                Some(vec![
                    traffic("membership-a", 100, 200),
                    traffic("membership-b", 300, 400),
                ]),
                runtime(Vec::new()),
            )
            .await;
        handle
            .record_local_sample(
                t1,
                "node-a",
                Some(vec![traffic("membership-a", 110, 220)]),
                runtime(Vec::new()),
            )
            .await;
        handle
            .record_local_sample(
                t2,
                "node-a",
                Some(vec![
                    traffic("membership-a", 120, 240),
                    traffic("membership-b", 330, 460),
                ]),
                runtime(Vec::new()),
            )
            .await;

        let snapshot = handle.snapshot("node-a").await.unwrap();
        assert_eq!(snapshot.daily_traffic[0].uplink_bytes, 20);
        assert_eq!(snapshot.daily_traffic[0].downlink_bytes, 40);
    }

    #[tokio::test]
    async fn sync_error_without_prior_snapshot_keeps_history_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let now = "2026-05-20T00:00:00Z".parse::<DateTime<Utc>>().unwrap();

        handle
            .mark_sync_error(now, "node-a", "request timeout".to_string())
            .await;

        assert!(handle.snapshot("node-a").await.is_none());
    }

    #[tokio::test]
    async fn prunes_events_to_seven_days_and_fifty_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let now = "2026-05-20T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let events = (0..80)
            .map(|i| NodeRuntimeEvent {
                event_id: format!("evt-{i}"),
                occurred_at: xp_test_fixtures::timestamp_at20260520_t000000_z().to_owned(),
                component: RuntimeComponent::Xray,
                kind: NodeRuntimeEventKind::StatusChanged,
                message: "xray status changed".to_string(),
                from_status: Some(RuntimeStatus::Up),
                to_status: Some(RuntimeStatus::Down),
            })
            .collect();

        handle
            .record_local_sample(
                now,
                "node-a",
                Some(vec![traffic("membership-a", 0, 0)]),
                runtime(events),
            )
            .await;

        let snapshot = handle.snapshot("node-a").await.unwrap();
        assert_eq!(snapshot.component_status_events.len(), 50);
        assert_eq!(snapshot.component_status_events[0].event_id, "evt-0");
    }
}
