use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use chrono::{DateTime, Days, SecondsFormat, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{Mutex, RwLock},
    time::MissedTickBehavior,
};
use tracing::warn;

use crate::{
    config::Config,
    domain::Node,
    internal_auth,
    node_runtime::{
        LocalNodeRuntimeSnapshot, NodeRuntimeEventKind, NodeRuntimeHandle, RuntimeComponent,
        RuntimeStatus,
    },
    state::{JsonSnapshotStore, membership_xray_email},
    xray,
};

const HISTORY_SCHEMA_VERSION: u32 = 1;
const HISTORY_WINDOW_DAYS: u64 = 90;
const EVENT_WINDOW_DAYS: u64 = 7;
const MAX_EVENTS_PER_NODE: usize = 50;
const SYNC_INTERVAL_SECS: u64 = 60 * 60;
const REMOTE_SYNC_TIMEOUT: Duration = Duration::from_secs(10);

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
        }
    }

    fn snapshot(&self) -> NodeHistorySnapshot {
        NodeHistorySnapshot {
            node_id: self.node_id.clone(),
            last_synced_at: self.last_synced_at.clone(),
            last_sync_error: self.last_sync_error.clone(),
            daily_traffic: self.daily_traffic.values().cloned().collect(),
            daily_component_status: self.daily_component_status.values().cloned().collect(),
            component_status_events: self.component_status_events.clone(),
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
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedNodeHistoryCache {
    schema_version: u32,
    #[serde(default)]
    nodes: BTreeMap<String, PersistedNodeHistoryRecord>,
}

impl PersistedNodeHistoryCache {
    fn empty() -> Self {
        Self {
            schema_version: HISTORY_SCHEMA_VERSION,
            nodes: BTreeMap::new(),
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
        {
            let mut state = self.inner.write().await;
            let record = state
                .nodes
                .entry(node_id.to_string())
                .or_insert_with(|| PersistedNodeHistoryRecord::empty(node_id.to_string()));
            record.last_synced_at = Some(rfc3339(now));
            record.last_sync_error = None;

            if let Some(totals) = traffic_totals {
                record_daily_traffic(record, now, totals);
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
            record.prune(now);
            state.nodes.insert(node_id.to_string(), record);
        }
        self.persist().await;
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
    pub uplink_total: u64,
    pub downlink_total: u64,
}

fn record_daily_traffic(
    record: &mut PersistedNodeHistoryRecord,
    now: DateTime<Utc>,
    totals: Vec<NodeTrafficTotals>,
) {
    let now_str = rfc3339(now);
    let date = date_key(now);
    let entry =
        record
            .daily_traffic
            .entry(date.clone())
            .or_insert_with(|| NodeHistoryDailyTraffic {
                date,
                uplink_bytes: 0,
                downlink_bytes: 0,
                updated_at: now_str.clone(),
            });

    for totals in totals {
        if let Some(previous) = record.traffic_baselines.get(&totals.membership_key)
            && totals.uplink_total >= previous.uplink_total
            && totals.downlink_total >= previous.downlink_total
        {
            entry.uplink_bytes = entry
                .uplink_bytes
                .saturating_add(totals.uplink_total - previous.uplink_total);
            entry.downlink_bytes = entry
                .downlink_bytes
                .saturating_add(totals.downlink_total - previous.downlink_total);
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
    entry.updated_at = now_str;
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
    components.sort_by(|a, b| a.component.cmp(&b.component));
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
        let mut ticker = tokio::time::interval(Duration::from_secs(SYNC_INTERVAL_SECS));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let now = Utc::now();
            let totals = collect_local_traffic_totals(&config, &store, &local_node_id).await;
            let runtime_snapshot = runtime.snapshot(MAX_EVENTS_PER_NODE).await;
            history
                .record_local_sample(now, &local_node_id, totals, runtime_snapshot)
                .await;
        }
    })
}

pub fn spawn_node_history_remote_sync_worker(
    local_node_id: String,
    store: Arc<Mutex<JsonSnapshotStore>>,
    history: NodeHistoryHandle,
    cluster_ca_pem: String,
    cluster_ca_key_pem: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let client = match build_cluster_http_client(&cluster_ca_pem) {
            Ok(client) => client,
            Err(err) => {
                warn!(%err, "node history remote sync disabled");
                return;
            }
        };
        let mut ticker = tokio::time::interval(Duration::from_secs(SYNC_INTERVAL_SECS));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let nodes = {
                let store = store.lock().await;
                store.list_nodes()
            };
            sync_remote_node_histories(
                &client,
                &cluster_ca_key_pem,
                &history,
                &local_node_id,
                nodes,
            )
            .await;
        }
    })
}

async fn collect_local_traffic_totals(
    config: &Config,
    store: &Arc<Mutex<JsonSnapshotStore>>,
    local_node_id: &str,
) -> Option<Vec<NodeTrafficTotals>> {
    let memberships = {
        let store = store.lock().await;
        store
            .state()
            .node_user_endpoint_memberships
            .iter()
            .filter(|membership| membership.node_id == local_node_id)
            .map(|membership| membership_xray_email(&membership.user_id, &membership.endpoint_id))
            .collect::<Vec<_>>()
    };
    if memberships.is_empty() {
        return Some(Vec::new());
    }

    let mut client = match xray::connect(config.xray_api_addr).await {
        Ok(client) => client,
        Err(err) => {
            warn!(%err, "node history traffic sample skipped: xray connect failed");
            return None;
        }
    };

    let mut out = Vec::new();
    for email in memberships {
        match client.get_user_traffic_totals(&email).await {
            Ok((uplink, downlink)) => {
                out.push(NodeTrafficTotals {
                    membership_key: email,
                    uplink_total: uplink,
                    downlink_total: downlink,
                });
            }
            Err(err) => {
                warn!(email, %err, "node history traffic stat skipped");
            }
        }
    }
    Some(out)
}

async fn sync_remote_node_histories(
    client: &Client,
    ca_key_pem: &str,
    history: &NodeHistoryHandle,
    local_node_id: &str,
    nodes: Vec<Node>,
) {
    for node in nodes {
        if node.node_id == local_node_id {
            continue;
        }
        let now = Utc::now();
        let base = node.api_base_url.trim_end_matches('/');
        if base.is_empty() {
            history
                .mark_sync_error(now, &node.node_id, "node api_base_url is empty".to_string())
                .await;
            continue;
        }

        match fetch_remote_history(client, ca_key_pem, base).await {
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

async fn fetch_remote_history(
    client: &Client,
    ca_key_pem: &str,
    base: &str,
) -> anyhow::Result<NodeHistorySnapshot> {
    let uri: axum::http::Uri = "/_internal/nodes/history/local".parse().expect("valid uri");
    let sig = internal_auth::sign_request(ca_key_pem, &axum::http::Method::GET, &uri)
        .map_err(|err| anyhow::anyhow!("sign internal request: {err}"))?;
    let request = client
        .get(format!("{base}/api/admin/_internal/nodes/history/local"))
        .header(
            reqwest::header::HeaderName::from_static(internal_auth::INTERNAL_SIGNATURE_HEADER),
            sig,
        )
        .send();
    let response = tokio::time::timeout(REMOTE_SYNC_TIMEOUT, request)
        .await
        .context("request timeout")??;
    if !response.status().is_success() {
        anyhow::bail!("node history request failed: {}", response.status());
    }
    response
        .json::<NodeHistorySnapshot>()
        .await
        .context("decode node history response")
}

fn build_cluster_http_client(cluster_ca_pem: &str) -> anyhow::Result<Client> {
    let ca = reqwest::Certificate::from_pem(cluster_ca_pem.as_bytes())
        .context("parse cluster ca pem")?;
    Client::builder()
        .add_root_certificate(ca)
        .danger_accept_invalid_hostnames(true)
        .build()
        .context("build cluster reqwest client")
}

fn load_history_cache(path: &Path) -> Option<PersistedNodeHistoryCache> {
    let bytes = fs::read(path).ok()?;
    let cache: PersistedNodeHistoryCache = serde_json::from_slice(&bytes).ok()?;
    if cache.schema_version != HISTORY_SCHEMA_VERSION {
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
            last_ok_at: None,
            last_fail_at: None,
            down_since: None,
            consecutive_failures: 0,
            recoveries_observed: 0,
            restart_attempts: 0,
            last_restart_at: None,
            last_restart_fail_at: None,
            last_sync_at: None,
            current_ipv4: None,
            current_ipv6: None,
            fast_mode_until: None,
            last_error: None,
        }
    }

    fn runtime(events: Vec<NodeRuntimeEvent>) -> LocalNodeRuntimeSnapshot {
        LocalNodeRuntimeSnapshot {
            node_id: "node-a".to_string(),
            summary: NodeRuntimeSummary {
                status: RuntimeSummaryStatus::Up,
                updated_at: "2026-05-20T00:00:00Z".to_string(),
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
            uplink_total,
            downlink_total,
        }
    }

    #[tokio::test]
    async fn records_daily_traffic_delta_and_component_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let t0 = "2026-05-20T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let t1 = "2026-05-20T01:00:00Z".parse::<DateTime<Utc>>().unwrap();

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
    async fn counter_reset_updates_baseline_without_negative_delta() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let t0 = "2026-05-20T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let t1 = "2026-05-20T01:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let t2 = "2026-05-20T02:00:00Z".parse::<DateTime<Utc>>().unwrap();

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
    async fn missing_membership_sample_does_not_advance_its_baseline() {
        let tmp = tempfile::tempdir().unwrap();
        let handle = NodeHistoryHandle::new(tmp.path().join("node_history_cache.json"));
        let t0 = "2026-05-20T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let t1 = "2026-05-20T01:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let t2 = "2026-05-20T02:00:00Z".parse::<DateTime<Utc>>().unwrap();

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
        assert_eq!(snapshot.daily_traffic[0].uplink_bytes, 50);
        assert_eq!(snapshot.daily_traffic[0].downlink_bytes, 100);
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
                occurred_at: rfc3339(now - chrono::Duration::hours(i)),
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
