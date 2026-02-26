use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use chrono::{DateTime, Duration as ChronoDuration, SecondsFormat, Timelike as _, Utc};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{RwLock, broadcast},
    time::MissedTickBehavior,
};
use tracing::warn;

use crate::{
    cloudflared_supervisor::{
        CloudflaredHealthHandle, CloudflaredHealthSnapshot, CloudflaredStatus,
    },
    config::{Config, XrayRestartMode},
    id::new_ulid_string,
    xray_supervisor::{XrayHealthHandle, XrayHealthSnapshot, XrayStatus},
};

const RUNTIME_SCHEMA_VERSION: u32 = 1;
const SLOT_WINDOW: usize = 7 * 24 * 2; // 7 days, 30 minutes per slot
const EVENT_WINDOW_DAYS: i64 = 7;
const MAX_EVENTS: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeComponent {
    Xp,
    Xray,
    Cloudflared,
}

impl RuntimeComponent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Xp => "xp",
            Self::Xray => "xray",
            Self::Cloudflared => "cloudflared",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStatus {
    Disabled,
    Up,
    Down,
    Unknown,
}

impl RuntimeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Up => "up",
            Self::Down => "down",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSummaryStatus {
    Up,
    Degraded,
    Down,
    Unknown,
}

impl RuntimeSummaryStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Degraded => "degraded",
            Self::Down => "down",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeRuntimeEventKind {
    StatusChanged,
    RestartRequested,
    RestartSucceeded,
    RestartFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRuntimeSummary {
    pub status: RuntimeSummaryStatus,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentRuntimeStatus {
    pub component: RuntimeComponent,
    pub status: RuntimeStatus,
    pub last_ok_at: Option<String>,
    pub last_fail_at: Option<String>,
    pub down_since: Option<String>,
    pub consecutive_failures: u32,
    pub recoveries_observed: u64,
    pub restart_attempts: u64,
    pub last_restart_at: Option<String>,
    pub last_restart_fail_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRuntimeHistorySlot {
    pub slot_start: String,
    pub status: RuntimeSummaryStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRuntimeEvent {
    pub event_id: String,
    pub occurred_at: String,
    pub component: RuntimeComponent,
    pub kind: NodeRuntimeEventKind,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_status: Option<RuntimeStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_status: Option<RuntimeStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalNodeRuntimeSnapshot {
    pub node_id: String,
    pub summary: NodeRuntimeSummary,
    pub components: Vec<ComponentRuntimeStatus>,
    pub recent_slots: Vec<NodeRuntimeHistorySlot>,
    pub events: Vec<NodeRuntimeEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedRuntime {
    schema_version: u32,
    node_id: String,
    components: Vec<ComponentRuntimeStatus>,
    slots: Vec<NodeRuntimeHistorySlot>,
    events: Vec<NodeRuntimeEvent>,
}

#[derive(Debug, Clone)]
struct NodeRuntimeState {
    node_id: String,
    summary: NodeRuntimeSummary,
    components: BTreeMap<RuntimeComponent, ComponentRuntimeStatus>,
    slot_statuses: BTreeMap<String, RuntimeSummaryStatus>,
    events: VecDeque<NodeRuntimeEvent>,
}

impl NodeRuntimeState {
    fn new(node_id: String, cloudflared_enabled: bool) -> Self {
        let now = Utc::now();
        let now_str = rfc3339(now);
        let mut components = BTreeMap::new();
        components.insert(
            RuntimeComponent::Xp,
            ComponentRuntimeStatus {
                component: RuntimeComponent::Xp,
                status: RuntimeStatus::Up,
                last_ok_at: Some(now_str.clone()),
                last_fail_at: None,
                down_since: None,
                consecutive_failures: 0,
                recoveries_observed: 0,
                restart_attempts: 0,
                last_restart_at: None,
                last_restart_fail_at: None,
            },
        );
        components.insert(
            RuntimeComponent::Xray,
            ComponentRuntimeStatus {
                component: RuntimeComponent::Xray,
                status: RuntimeStatus::Unknown,
                last_ok_at: None,
                last_fail_at: None,
                down_since: None,
                consecutive_failures: 0,
                recoveries_observed: 0,
                restart_attempts: 0,
                last_restart_at: None,
                last_restart_fail_at: None,
            },
        );
        components.insert(
            RuntimeComponent::Cloudflared,
            ComponentRuntimeStatus {
                component: RuntimeComponent::Cloudflared,
                status: if cloudflared_enabled {
                    RuntimeStatus::Unknown
                } else {
                    RuntimeStatus::Disabled
                },
                last_ok_at: None,
                last_fail_at: None,
                down_since: None,
                consecutive_failures: 0,
                recoveries_observed: 0,
                restart_attempts: 0,
                last_restart_at: None,
                last_restart_fail_at: None,
            },
        );

        let summary_status = compute_summary(components.values().map(|item| item.status));

        Self {
            node_id,
            summary: NodeRuntimeSummary {
                status: summary_status,
                updated_at: now_str,
            },
            components,
            slot_statuses: BTreeMap::new(),
            events: VecDeque::new(),
        }
    }

    fn load_from(
        persisted: PersistedRuntime,
        cloudflared_enabled: bool,
        now: DateTime<Utc>,
    ) -> Option<Self> {
        if persisted.schema_version != RUNTIME_SCHEMA_VERSION {
            return None;
        }

        let mut state = Self::new(persisted.node_id, cloudflared_enabled);
        for component in persisted.components {
            state.components.insert(component.component, component);
        }
        for slot in persisted.slots {
            state.slot_statuses.insert(slot.slot_start, slot.status);
        }
        for event in persisted.events {
            state.events.push_back(event);
        }
        state.prune(now);
        state.recompute_summary(now);
        Some(state)
    }

    fn to_persisted(&self) -> PersistedRuntime {
        PersistedRuntime {
            schema_version: RUNTIME_SCHEMA_VERSION,
            node_id: self.node_id.clone(),
            components: self.components.values().cloned().collect(),
            slots: self
                .slot_statuses
                .iter()
                .map(|(slot_start, status)| NodeRuntimeHistorySlot {
                    slot_start: slot_start.clone(),
                    status: *status,
                })
                .collect(),
            events: self.events.iter().cloned().collect(),
        }
    }

    fn recompute_summary(&mut self, now: DateTime<Utc>) -> bool {
        let next = compute_summary(self.components.values().map(|item| item.status));
        if self.summary.status != next {
            self.summary.status = next;
            self.summary.updated_at = rfc3339(now);
            return true;
        }
        false
    }

    fn record_slot(&mut self, now: DateTime<Utc>) -> bool {
        let slot_start = slot_key(now);
        match self.slot_statuses.get(&slot_start).copied() {
            Some(current) if current == self.summary.status => false,
            _ => {
                self.slot_statuses.insert(slot_start, self.summary.status);
                true
            }
        }
    }

    fn prune(&mut self, now: DateTime<Utc>) {
        let slot_cutoff = slot_key(now - ChronoDuration::days(EVENT_WINDOW_DAYS));
        while let Some((key, _)) = self.slot_statuses.first_key_value() {
            if key >= &slot_cutoff {
                break;
            }
            let stale = key.clone();
            self.slot_statuses.remove(&stale);
        }
        while self.slot_statuses.len() > SLOT_WINDOW {
            let Some((key, _)) = self.slot_statuses.first_key_value() else {
                break;
            };
            let stale = key.clone();
            self.slot_statuses.remove(&stale);
        }

        let event_cutoff = rfc3339(now - ChronoDuration::days(EVENT_WINDOW_DAYS));
        while let Some(last) = self.events.back() {
            if last.occurred_at >= event_cutoff {
                break;
            }
            self.events.pop_back();
        }
        while self.events.len() > MAX_EVENTS {
            self.events.pop_back();
        }
    }

    fn recent_slots(&self, now: DateTime<Utc>) -> Vec<NodeRuntimeHistorySlot> {
        let mut slots = Vec::with_capacity(SLOT_WINDOW);
        let current = truncate_to_half_hour(now);
        for i in (0..SLOT_WINDOW).rev() {
            let at = current - ChronoDuration::minutes((i as i64) * 30);
            let key = rfc3339(at);
            slots.push(NodeRuntimeHistorySlot {
                slot_start: key.clone(),
                status: self
                    .slot_statuses
                    .get(&key)
                    .copied()
                    .unwrap_or(RuntimeSummaryStatus::Unknown),
            });
        }
        slots
    }
}

#[derive(Clone)]
pub struct NodeRuntimeHandle {
    inner: Arc<RwLock<NodeRuntimeState>>,
    events_tx: broadcast::Sender<NodeRuntimeEvent>,
    persistence_path: Arc<PathBuf>,
}

impl NodeRuntimeHandle {
    pub fn from_config(config: &Config, node_id: String) -> Self {
        let cloudflared_enabled = config.cloudflared_restart_mode != XrayRestartMode::None;
        Self::new(
            config.data_dir.join("service_runtime.json"),
            node_id,
            cloudflared_enabled,
        )
    }

    fn new(persistence_path: PathBuf, node_id: String, cloudflared_enabled: bool) -> Self {
        let now = Utc::now();
        let (events_tx, _) = broadcast::channel::<NodeRuntimeEvent>(512);

        let state = load_persisted_state(&persistence_path, cloudflared_enabled, now)
            .unwrap_or_else(|| NodeRuntimeState::new(node_id, cloudflared_enabled));

        Self {
            inner: Arc::new(RwLock::new(state)),
            events_tx,
            persistence_path: Arc::new(persistence_path),
        }
    }

    pub async fn snapshot(&self, event_limit: usize) -> LocalNodeRuntimeSnapshot {
        let now = Utc::now();
        let state = self.inner.read().await;
        LocalNodeRuntimeSnapshot {
            node_id: state.node_id.clone(),
            summary: state.summary.clone(),
            components: state.components.values().cloned().collect(),
            recent_slots: state.recent_slots(now),
            events: state.events.iter().take(event_limit).cloned().collect(),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<NodeRuntimeEvent> {
        self.events_tx.subscribe()
    }

    pub async fn apply_probe_snapshots(
        &self,
        now: DateTime<Utc>,
        xray: XrayHealthSnapshot,
        cloudflared: CloudflaredHealthSnapshot,
    ) {
        let xray_component = map_xray_component(xray);
        let cloudflared_component = map_cloudflared_component(cloudflared);

        let mut events_to_emit: Vec<NodeRuntimeEvent> = Vec::new();
        let mut should_persist = false;

        {
            let mut state = self.inner.write().await;

            should_persist |=
                apply_component_update(&mut state, xray_component, now, &mut events_to_emit);
            should_persist |=
                apply_component_update(&mut state, cloudflared_component, now, &mut events_to_emit);

            if state.recompute_summary(now) {
                should_persist = true;
            }
            if state.record_slot(now) {
                should_persist = true;
            }

            for event in &events_to_emit {
                state.events.push_front(event.clone());
            }
            state.prune(now);
        }

        for event in events_to_emit {
            let _ = self.events_tx.send(event);
        }

        if should_persist {
            self.persist().await;
        }
    }

    async fn persist(&self) {
        let persisted = {
            let state = self.inner.read().await;
            state.to_persisted()
        };
        if let Err(err) = persist_runtime(&self.persistence_path, &persisted) {
            warn!(error = %err, path = %self.persistence_path.display(), "persist service runtime");
        }
    }

    #[cfg(test)]
    fn test_new(path: PathBuf, node_id: String, cloudflared_enabled: bool) -> Self {
        Self::new(path, node_id, cloudflared_enabled)
    }
}

pub fn spawn_node_runtime_monitor(
    config: Arc<Config>,
    node_id: String,
    xray_health: XrayHealthHandle,
    cloudflared_health: CloudflaredHealthHandle,
) -> (NodeRuntimeHandle, tokio::task::JoinHandle<()>) {
    let runtime = NodeRuntimeHandle::from_config(&config, node_id);
    let runtime_clone = runtime.clone();

    let task = tokio::spawn(async move {
        let interval_secs = std::cmp::max(
            1,
            std::cmp::min(
                config.xray_health_interval_secs,
                config.cloudflared_health_interval_secs,
            ),
        );
        let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            ticker.tick().await;
            let now = Utc::now();
            let xray = xray_health.snapshot().await;
            let cloudflared = cloudflared_health.snapshot().await;
            runtime_clone
                .apply_probe_snapshots(now, xray, cloudflared)
                .await;
        }
    });

    (runtime, task)
}

fn apply_component_update(
    state: &mut NodeRuntimeState,
    next: ComponentRuntimeStatus,
    now: DateTime<Utc>,
    events: &mut Vec<NodeRuntimeEvent>,
) -> bool {
    let previous = state.components.get(&next.component).cloned();
    state.components.insert(next.component, next.clone());
    let Some(previous) = previous else {
        return true;
    };

    let mut should_persist = false;
    if previous.status != next.status {
        should_persist = true;
        events.push(NodeRuntimeEvent {
            event_id: new_ulid_string(),
            occurred_at: rfc3339(now),
            component: next.component,
            kind: NodeRuntimeEventKind::StatusChanged,
            message: format!(
                "{} status changed: {} -> {}",
                next.component.as_str(),
                previous.status.as_str(),
                next.status.as_str()
            ),
            from_status: Some(previous.status),
            to_status: Some(next.status),
        });
    }

    if next.restart_attempts > previous.restart_attempts {
        should_persist = true;
        let restart_time = next.last_restart_at.clone().unwrap_or_else(|| rfc3339(now));
        let restart_fail = next.last_restart_fail_at.as_ref() == Some(&restart_time);

        events.push(NodeRuntimeEvent {
            event_id: new_ulid_string(),
            occurred_at: restart_time.clone(),
            component: next.component,
            kind: NodeRuntimeEventKind::RestartRequested,
            message: format!("{} restart requested", next.component.as_str()),
            from_status: None,
            to_status: Some(next.status),
        });
        events.push(NodeRuntimeEvent {
            event_id: new_ulid_string(),
            occurred_at: restart_time,
            component: next.component,
            kind: if restart_fail {
                NodeRuntimeEventKind::RestartFailed
            } else {
                NodeRuntimeEventKind::RestartSucceeded
            },
            message: if restart_fail {
                format!("{} restart request failed", next.component.as_str())
            } else {
                format!("{} restart request accepted", next.component.as_str())
            },
            from_status: None,
            to_status: Some(next.status),
        });
    }

    should_persist
}

fn map_xray_component(snapshot: XrayHealthSnapshot) -> ComponentRuntimeStatus {
    ComponentRuntimeStatus {
        component: RuntimeComponent::Xray,
        status: match snapshot.status {
            XrayStatus::Unknown => RuntimeStatus::Unknown,
            XrayStatus::Up => RuntimeStatus::Up,
            XrayStatus::Down => RuntimeStatus::Down,
        },
        last_ok_at: snapshot.last_ok_at.map(rfc3339),
        last_fail_at: snapshot.last_fail_at.map(rfc3339),
        down_since: snapshot.down_since.map(rfc3339),
        consecutive_failures: snapshot.consecutive_failures,
        recoveries_observed: snapshot.recoveries_observed,
        restart_attempts: snapshot.restart_attempts,
        last_restart_at: snapshot.last_restart_at.map(rfc3339),
        last_restart_fail_at: snapshot.last_restart_fail_at.map(rfc3339),
    }
}

fn map_cloudflared_component(snapshot: CloudflaredHealthSnapshot) -> ComponentRuntimeStatus {
    ComponentRuntimeStatus {
        component: RuntimeComponent::Cloudflared,
        status: match snapshot.status {
            CloudflaredStatus::Disabled => RuntimeStatus::Disabled,
            CloudflaredStatus::Unknown => RuntimeStatus::Unknown,
            CloudflaredStatus::Up => RuntimeStatus::Up,
            CloudflaredStatus::Down => RuntimeStatus::Down,
        },
        last_ok_at: snapshot.last_ok_at.map(rfc3339),
        last_fail_at: snapshot.last_fail_at.map(rfc3339),
        down_since: snapshot.down_since.map(rfc3339),
        consecutive_failures: snapshot.consecutive_failures,
        recoveries_observed: snapshot.recoveries_observed,
        restart_attempts: snapshot.restart_attempts,
        last_restart_at: snapshot.last_restart_at.map(rfc3339),
        last_restart_fail_at: snapshot.last_restart_fail_at.map(rfc3339),
    }
}

fn compute_summary<I>(statuses: I) -> RuntimeSummaryStatus
where
    I: IntoIterator<Item = RuntimeStatus>,
{
    let statuses: Vec<_> = statuses
        .into_iter()
        .filter(|status| *status != RuntimeStatus::Disabled)
        .collect();
    if statuses.is_empty() {
        return RuntimeSummaryStatus::Unknown;
    }

    let all_up = statuses.iter().all(|status| *status == RuntimeStatus::Up);
    if all_up {
        return RuntimeSummaryStatus::Up;
    }

    let all_down = statuses.iter().all(|status| *status == RuntimeStatus::Down);
    if all_down {
        return RuntimeSummaryStatus::Down;
    }

    if statuses.contains(&RuntimeStatus::Down) {
        return RuntimeSummaryStatus::Degraded;
    }

    if statuses.contains(&RuntimeStatus::Unknown) {
        return RuntimeSummaryStatus::Unknown;
    }

    RuntimeSummaryStatus::Degraded
}

fn truncate_to_half_hour(at: DateTime<Utc>) -> DateTime<Utc> {
    let minute = if at.minute() < 30 { 0 } else { 30 };
    at.with_minute(minute)
        .and_then(|value| value.with_second(0))
        .and_then(|value| value.with_nanosecond(0))
        .unwrap_or(at)
}

fn slot_key(at: DateTime<Utc>) -> String {
    rfc3339(truncate_to_half_hour(at))
}

fn rfc3339(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn load_persisted_state(
    path: &PathBuf,
    cloudflared_enabled: bool,
    now: DateTime<Utc>,
) -> Option<NodeRuntimeState> {
    let raw = fs::read(path).ok()?;
    let parsed = serde_json::from_slice::<PersistedRuntime>(&raw).ok()?;
    NodeRuntimeState::load_from(parsed, cloudflared_enabled, now)
}

fn persist_runtime(path: &PathBuf, payload: &PersistedRuntime) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create parent dir: {e}"))?;
    }

    let bytes = serde_json::to_vec_pretty(payload).map_err(|e| format!("serialize: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes).map_err(|e| format!("write tmp: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| format!("rename tmp: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    #[test]
    fn summary_rules_are_stable() {
        assert_eq!(
            compute_summary([RuntimeStatus::Up, RuntimeStatus::Up]),
            RuntimeSummaryStatus::Up
        );
        assert_eq!(
            compute_summary([RuntimeStatus::Down, RuntimeStatus::Down]),
            RuntimeSummaryStatus::Down
        );
        assert_eq!(
            compute_summary([RuntimeStatus::Up, RuntimeStatus::Down]),
            RuntimeSummaryStatus::Degraded
        );
        assert_eq!(
            compute_summary([RuntimeStatus::Unknown, RuntimeStatus::Up]),
            RuntimeSummaryStatus::Unknown
        );
        assert_eq!(
            compute_summary([RuntimeStatus::Disabled]),
            RuntimeSummaryStatus::Unknown
        );
    }

    #[tokio::test]
    async fn status_transition_and_restart_events_are_recorded() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("service_runtime.json");
        let handle = NodeRuntimeHandle::test_new(path, "node-1".to_string(), true);

        let mut xray = XrayHealthSnapshot {
            status: XrayStatus::Up,
            ..XrayHealthSnapshot::default()
        };
        let cloudflared = CloudflaredHealthSnapshot {
            status: CloudflaredStatus::Up,
            ..CloudflaredHealthSnapshot::default()
        };

        handle
            .apply_probe_snapshots(Utc::now(), xray.clone(), cloudflared.clone())
            .await;

        xray.status = XrayStatus::Down;
        xray.restart_attempts = 1;
        let restart_at = Utc::now();
        xray.last_restart_at = Some(restart_at);
        handle
            .apply_probe_snapshots(Utc::now(), xray.clone(), cloudflared.clone())
            .await;

        let snapshot = handle.snapshot(20).await;
        assert!(
            snapshot
                .events
                .iter()
                .any(|event| event.kind == NodeRuntimeEventKind::StatusChanged
                    && event.component == RuntimeComponent::Xray)
        );
        assert!(
            snapshot
                .events
                .iter()
                .any(|event| event.kind == NodeRuntimeEventKind::RestartRequested
                    && event.component == RuntimeComponent::Xray)
        );
        assert_eq!(snapshot.recent_slots.len(), SLOT_WINDOW);
    }

    #[tokio::test]
    async fn persisted_state_is_restored() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("service_runtime.json");

        let handle = NodeRuntimeHandle::test_new(path.clone(), "node-1".to_string(), false);
        handle
            .apply_probe_snapshots(
                Utc::now(),
                XrayHealthSnapshot {
                    status: XrayStatus::Down,
                    ..XrayHealthSnapshot::default()
                },
                CloudflaredHealthSnapshot {
                    status: CloudflaredStatus::Disabled,
                    ..CloudflaredHealthSnapshot::default()
                },
            )
            .await;

        let before = handle.snapshot(10).await;
        assert!(!before.events.is_empty());

        let restored = NodeRuntimeHandle::test_new(path, "node-1".to_string(), false);
        let after = restored.snapshot(10).await;
        assert!(!after.events.is_empty());
        assert_eq!(after.node_id, "node-1");
    }
}
