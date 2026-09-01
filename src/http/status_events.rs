use std::{
    collections::{BTreeMap, VecDeque},
    convert::Infallible,
    future::Future,
    sync::{Arc, Mutex as StdMutex},
};

use axum::response::sse::Event;
use futures_util::{Stream, stream};
use tokio::{
    sync::{Notify, broadcast},
    time::Duration,
};

use super::{
    AdminStatusSnapshotError, AppState, admin_status_snapshot_fingerprint,
    build_admin_status_snapshot, sse_json_event,
};

#[derive(Clone)]
pub(super) enum StatusEventUpdate {
    Snapshot(String),
    SnapshotError(AdminStatusSnapshotError),
    ResourceAlert {
        event_type: &'static str,
        alert: Box<super::AlertItem>,
    },
}

impl StatusEventUpdate {
    pub(super) fn into_sse_event(self) -> Event {
        match self {
            Self::Snapshot(snapshot) => Event::default().event("snapshot").data(snapshot),
            Self::SnapshotError(error) => sse_json_event("snapshot_error", &error),
            Self::ResourceAlert { event_type, alert } => sse_json_event(event_type, &alert),
        }
    }
}

#[derive(Default)]
struct StatusEventsLifecycle {
    subscribers: usize,
    worker_running: bool,
    worker_generation: u64,
    worker_starter: Option<StatusEventsWorkerStarter>,
    latest_update: Option<StatusEventUpdate>,
    refresh_requested: bool,
}

type StatusEventsWorkerStarter = Arc<dyn Fn(StatusEventsWorkerGuard) + Send + Sync>;

#[derive(Clone)]
pub(super) struct StatusEventsHub {
    sender: broadcast::Sender<StatusEventUpdate>,
    lifecycle: Arc<StdMutex<StatusEventsLifecycle>>,
    shutdown: Arc<Notify>,
}

pub(super) struct StatusEventsSubscription {
    pub(super) receiver: broadcast::Receiver<StatusEventUpdate>,
    pub(super) replay: Option<StatusEventUpdate>,
    hub: StatusEventsHub,
}

pub(super) struct StatusEventsWorkerGuard {
    hub: StatusEventsHub,
    generation: u64,
    disarmed: bool,
}

impl StatusEventsWorkerGuard {
    fn disarm(&mut self) {
        self.disarmed = true;
    }
}

impl Drop for StatusEventsWorkerGuard {
    fn drop(&mut self) {
        if !self.disarmed {
            self.hub.worker_exited(self.generation);
        }
    }
}

impl Drop for StatusEventsSubscription {
    fn drop(&mut self) {
        self.hub.unsubscribe();
    }
}

impl StatusEventsSubscription {
    pub(super) fn recover_after_lag(&mut self) -> Option<StatusEventUpdate> {
        let lifecycle = self
            .hub
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.receiver = self.hub.sender.subscribe();
        lifecycle.latest_update.clone()
    }
}

impl StatusEventsHub {
    pub(super) fn new() -> Self {
        let (sender, _) = broadcast::channel(32);
        Self {
            sender,
            lifecycle: Arc::new(StdMutex::new(StatusEventsLifecycle::default())),
            shutdown: Arc::new(Notify::new()),
        }
    }

    pub(super) fn subscribe<F>(&self, start_worker: F) -> StatusEventsSubscription
    where
        F: Fn(StatusEventsWorkerGuard) + Send + Sync + 'static,
    {
        let start_worker: StatusEventsWorkerStarter = Arc::new(start_worker);
        let (receiver, replay, worker_guard, wake_worker) = {
            let mut lifecycle = self
                .lifecycle
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let receiver = self.sender.subscribe();
            lifecycle.subscribers += 1;
            let worker_guard = if lifecycle.worker_running {
                None
            } else {
                lifecycle.worker_running = true;
                lifecycle.worker_generation = lifecycle.worker_generation.saturating_add(1);
                lifecycle.worker_starter = Some(start_worker.clone());
                Some(StatusEventsWorkerGuard {
                    hub: self.clone(),
                    generation: lifecycle.worker_generation,
                    disarmed: false,
                })
            };
            let wake_worker = worker_guard.is_none() && lifecycle.refresh_requested;
            (
                receiver,
                lifecycle.latest_update.clone(),
                worker_guard,
                wake_worker,
            )
        };
        if let Some(worker_guard) = worker_guard {
            start_worker(worker_guard);
        } else if wake_worker {
            self.shutdown.notify_one();
        }
        StatusEventsSubscription {
            receiver,
            replay,
            hub: self.clone(),
        }
    }

    fn unsubscribe(&self) {
        let should_stop = {
            let mut lifecycle = self
                .lifecycle
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            lifecycle.subscribers = lifecycle.subscribers.saturating_sub(1);
            let is_inactive = lifecycle.subscribers == 0;
            if is_inactive {
                lifecycle.latest_update = None;
                lifecycle.refresh_requested = true;
            }
            is_inactive
        };
        if should_stop {
            self.shutdown.notify_one();
        }
    }

    fn stop_if_inactive(&self) -> bool {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifecycle.subscribers != 0 {
            return false;
        }
        lifecycle.worker_running = false;
        true
    }

    fn worker_exited(&self, generation: u64) {
        let replacement = {
            let mut lifecycle = self
                .lifecycle
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !lifecycle.worker_running || lifecycle.worker_generation != generation {
                return;
            }
            if lifecycle.subscribers == 0 {
                lifecycle.worker_running = false;
                return;
            }

            lifecycle.refresh_requested = true;
            lifecycle.worker_generation = lifecycle.worker_generation.saturating_add(1);
            let Some(start_worker) = lifecycle.worker_starter.clone() else {
                lifecycle.worker_running = false;
                return;
            };
            let worker_guard = StatusEventsWorkerGuard {
                hub: self.clone(),
                generation: lifecycle.worker_generation,
                disarmed: false,
            };
            Some((start_worker, worker_guard))
        };
        if let Some((start_worker, worker_guard)) = replacement {
            start_worker(worker_guard);
        }
    }

    #[cfg(test)]
    fn lifecycle_for_test(&self) -> (usize, bool) {
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (lifecycle.subscribers, lifecycle.worker_running)
    }

    pub(super) async fn run(self, state: AppState, worker_guard: StatusEventsWorkerGuard) {
        self.run_with_snapshot_builder(worker_guard, move || {
            let state = state.clone();
            async move {
                build_admin_status_snapshot(&state)
                    .await
                    .map_err(|error| error.message)
            }
        })
        .await;
    }

    async fn run_with_snapshot_builder<F, Fut>(
        self,
        mut worker_guard: StatusEventsWorkerGuard,
        mut build_snapshot: F,
    ) where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<super::AdminStatusSnapshot, String>>,
    {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        let mut last_snapshot_fingerprint: Option<String> = None;
        let mut last_resource_alerts: Option<BTreeMap<String, super::AlertItem>> = None;

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if self.stop_if_inactive() {
                        worker_guard.disarm();
                        return;
                    }
                    self.publish_built_snapshot(
                        build_snapshot().await,
                        &mut last_snapshot_fingerprint,
                        &mut last_resource_alerts,
                    );
                }
                _ = self.shutdown.notified() => {
                    if self.stop_if_inactive() {
                        worker_guard.disarm();
                        return;
                    }
                    self.publish_built_snapshot(
                        build_snapshot().await,
                        &mut last_snapshot_fingerprint,
                        &mut last_resource_alerts,
                    );
                }
            }
        }
    }

    fn publish_built_snapshot(
        &self,
        result: Result<super::AdminStatusSnapshot, String>,
        last_snapshot_fingerprint: &mut Option<String>,
        last_resource_alerts: &mut Option<BTreeMap<String, super::AlertItem>>,
    ) {
        match result {
            Ok(snapshot) => self.publish_serialized_snapshot(
                snapshot,
                last_snapshot_fingerprint,
                last_resource_alerts,
            ),
            Err(message) => self.publish_error(message, last_snapshot_fingerprint),
        }
    }

    fn publish_serialized_snapshot(
        &self,
        snapshot: super::AdminStatusSnapshot,
        last_snapshot_fingerprint: &mut Option<String>,
        last_resource_alerts: &mut Option<BTreeMap<String, super::AlertItem>>,
    ) {
        let resource_events = resource_alert_events(last_resource_alerts, &snapshot.alerts);
        let snapshot_json = match serde_json::to_string(&snapshot) {
            Ok(snapshot_json) => snapshot_json,
            Err(error) => {
                return self.publish_error(
                    format!("serialize status snapshot: {error}"),
                    last_snapshot_fingerprint,
                );
            }
        };
        match admin_status_snapshot_fingerprint(&snapshot) {
            Ok(fingerprint)
                if self.publish_snapshot_if_active(
                    StatusEventUpdate::Snapshot(snapshot_json),
                    &fingerprint,
                    last_snapshot_fingerprint,
                ) =>
            {
                *last_snapshot_fingerprint = Some(fingerprint);
            }
            Ok(_) => {}
            Err(error) => self.publish_error(
                format!("serialize status snapshot: {error}"),
                last_snapshot_fingerprint,
            ),
        }
        for event in resource_events {
            self.publish_update_without_replay(event);
        }
    }

    fn publish_error(&self, message: String, last_snapshot_fingerprint: &mut Option<String>) {
        *last_snapshot_fingerprint = None;
        self.publish_update_if_active(StatusEventUpdate::SnapshotError(AdminStatusSnapshotError {
            message,
        }));
    }

    fn publish_snapshot_if_active(
        &self,
        update: StatusEventUpdate,
        fingerprint: &str,
        last_snapshot_fingerprint: &Option<String>,
    ) -> bool {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifecycle.subscribers == 0 {
            return false;
        }
        if !lifecycle.refresh_requested && last_snapshot_fingerprint.as_deref() == Some(fingerprint)
        {
            return false;
        }
        lifecycle.refresh_requested = false;
        lifecycle.latest_update = Some(update.clone());
        let _ = self.sender.send(update);
        true
    }

    fn publish_update_if_active(&self, update: StatusEventUpdate) -> bool {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifecycle.subscribers == 0 {
            return false;
        }
        lifecycle.latest_update = Some(update.clone());
        lifecycle.refresh_requested = false;
        let _ = self.sender.send(update);
        true
    }

    fn publish_update_without_replay(&self, update: StatusEventUpdate) -> bool {
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifecycle.subscribers == 0 {
            return false;
        }
        let _ = self.sender.send(update);
        true
    }
}

fn resource_alert_key(alert: &super::AlertItem) -> Option<String> {
    alert.resource_node_id.as_ref().map(|node_id| {
        format!(
            "{}:{}:{}",
            node_id,
            alert.scope.as_deref().unwrap_or_default(),
            alert.metric.as_deref().unwrap_or_default()
        )
    })
}

fn resource_alert_events(
    previous: &mut Option<BTreeMap<String, super::AlertItem>>,
    alerts: &super::AlertsResponse,
) -> Vec<StatusEventUpdate> {
    let current = alerts
        .items
        .iter()
        .filter_map(|alert| resource_alert_key(alert).map(|key| (key, alert.clone())))
        .collect::<BTreeMap<_, _>>();
    let old = previous.replace(current.clone()).unwrap_or_default();
    let mut events = Vec::new();
    for (key, alert) in &current {
        let event_type = match old.get(key) {
            None => Some("resource_alert_opened"),
            Some(previous) if previous.severity.as_deref() != alert.severity.as_deref() => {
                Some("resource_alert_escalated")
            }
            Some(_) => None,
        };
        if let Some(event_type) = event_type {
            events.push(StatusEventUpdate::ResourceAlert {
                event_type,
                alert: Box::new(alert.clone()),
            });
        }
    }
    for (key, alert) in old {
        if !current.contains_key(&key) {
            events.push(StatusEventUpdate::ResourceAlert {
                event_type: "resource_alert_recovered",
                alert: Box::new(alert),
            });
        }
    }
    events
}

pub(super) fn stream_events(
    initial_events: VecDeque<Event>,
    subscription: StatusEventsSubscription,
) -> impl Stream<Item = Result<Event, Infallible>> + Send {
    stream::unfold(
        (initial_events, subscription),
        |(mut initial, mut subscription)| async move {
            if let Some(event) = initial.pop_front() {
                return Some((Ok(event), (initial, subscription)));
            }
            loop {
                match subscription.receiver.recv().await {
                    Ok(event) => {
                        return Some((Ok(event.into_sse_event()), (initial, subscription)));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if let Some(event) = subscription.recover_after_lag() {
                            return Some((Ok(event.into_sse_event()), (initial, subscription)));
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::AtomicUsize};

    use axum::response::{IntoResponse, sse::Sse};
    use futures_util::StreamExt;
    use http_body_util::BodyExt;

    use super::*;

    async fn event_text(event: Event) -> String {
        let response = Sse::new(futures_util::stream::once(async {
            Ok::<_, Infallible>(event)
        }))
        .into_response();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn discard_worker(mut worker_guard: StatusEventsWorkerGuard) {
        worker_guard.disarm();
    }

    fn status_snapshot_for_test() -> super::super::AdminStatusSnapshot {
        super::super::AdminStatusSnapshot {
            emitted_at: "2026-08-20T10:00:00Z".to_string(),
            health: super::super::AdminStatusHealthSnapshot { status: "ok" },
            cluster_info: super::super::ClusterInfoResponse {
                cluster_id: xp_test_fixtures::primary_cluster_id().to_owned(),
                node_id: xp_test_fixtures::primary_node_id().to_owned(),
                role: "leader",
                leader_api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
                term: 1,
                xp_version: "test".to_string(),
            },
            nodes_runtime: super::super::AdminNodesRuntimeResponse {
                partial: false,
                unreachable_nodes: vec![],
                items: vec![],
            },
            alerts: super::super::AlertsResponse {
                partial: false,
                unreachable_nodes: vec![],
                items: vec![],
            },
            upgrade: super::super::AdminUpgradeStatusResponse {
                support: crate::upgrade_job::UpgradeSupport {
                    supported: false,
                    reason: None,
                    trigger: None,
                    storage: None,
                },
                status: crate::upgrade_job::UpgradeJobStatus {
                    state: crate::upgrade_job::UpgradeJobState::Idle,
                    target_tag: None,
                    repo: None,
                    started_at: None,
                    finished_at: None,
                    exit_code: None,
                    message: None,
                    updated_at: xp_test_fixtures::baseline_timestamp().to_owned(),
                },
            },
            mesh_revision: 1,
        }
    }

    #[tokio::test]
    async fn shares_one_producer_and_stops_after_last_subscriber() {
        let hub = StatusEventsHub::new();
        let starts = Arc::new(AtomicUsize::new(0));
        let first_starts = starts.clone();
        let mut first = hub.subscribe(move |mut worker_guard| {
            first_starts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            worker_guard.disarm();
        });
        let second_starts = starts.clone();
        let mut second = hub.subscribe(move |mut worker_guard| {
            second_starts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            worker_guard.disarm();
        });

        assert_eq!(starts.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(hub.lifecycle_for_test(), (2, true));

        hub.publish_update_if_active(StatusEventUpdate::Snapshot(
            "{\"health\":\"ok\"}".to_string(),
        ));
        let first_update = first.receiver.recv().await.unwrap();
        let second_update = second.receiver.recv().await.unwrap();
        assert!(matches!(
            first_update,
            StatusEventUpdate::Snapshot(ref snapshot) if snapshot == "{\"health\":\"ok\"}"
        ));
        assert!(matches!(
            second_update,
            StatusEventUpdate::Snapshot(ref snapshot) if snapshot == "{\"health\":\"ok\"}"
        ));

        drop(first);
        assert_eq!(hub.lifecycle_for_test(), (1, true));
        drop(second);
        assert!(hub.stop_if_inactive());
        assert_eq!(hub.lifecycle_for_test(), (0, false));
    }

    #[tokio::test(start_paused = true)]
    async fn running_producer_fans_out_once_per_tick_and_stops_after_disconnect() {
        let hub = StatusEventsHub::new();
        let starts = Arc::new(AtomicUsize::new(0));
        let builds = Arc::new(AtomicUsize::new(0));
        let worker_hub = hub.clone();
        let worker_starts = starts.clone();
        let worker_builds = builds.clone();
        let mut first = hub.subscribe(move |worker_guard| {
            worker_starts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let worker_hub = worker_hub.clone();
            let worker_builds = worker_builds.clone();
            tokio::spawn(async move {
                worker_hub
                    .run_with_snapshot_builder(worker_guard, move || {
                        let builds = worker_builds.clone();
                        async move {
                            builds.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            Ok(status_snapshot_for_test())
                        }
                    })
                    .await;
            });
        });
        let second_starts = starts.clone();
        let mut second = hub.subscribe(move |mut worker_guard| {
            second_starts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            worker_guard.disarm();
        });

        tokio::task::yield_now().await;
        assert!(matches!(
            first.receiver.recv().await.unwrap(),
            StatusEventUpdate::Snapshot(_)
        ));
        assert!(matches!(
            second.receiver.recv().await.unwrap(),
            StatusEventUpdate::Snapshot(_)
        ));
        assert_eq!(starts.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(builds.load(std::sync::atomic::Ordering::Relaxed), 1);

        tokio::time::advance(Duration::from_secs(5)).await;
        tokio::task::yield_now().await;
        assert!(matches!(
            first.receiver.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
        assert!(matches!(
            second.receiver.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
        assert_eq!(builds.load(std::sync::atomic::Ordering::Relaxed), 2);

        drop(first);
        drop(second);
        tokio::task::yield_now().await;
        assert_eq!(hub.lifecycle_for_test(), (0, false));
    }

    #[tokio::test]
    async fn automatically_restarts_an_unexpected_worker_exit_for_existing_subscribers() {
        let hub = StatusEventsHub::new();
        let starts = Arc::new(AtomicUsize::new(0));
        let worker_hub = hub.clone();
        let worker_starts = starts.clone();
        let attempts = Arc::new(AtomicUsize::new(0));
        let worker_attempts = attempts.clone();
        let mut subscriber = hub.subscribe(move |worker_guard| {
            worker_starts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let attempt = worker_attempts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if attempt == 0 {
                return;
            }
            let worker_hub = worker_hub.clone();
            tokio::spawn(async move {
                worker_hub
                    .run_with_snapshot_builder(worker_guard, || async {
                        Ok(status_snapshot_for_test())
                    })
                    .await;
            });
        });

        let update = tokio::time::timeout(Duration::from_secs(1), subscriber.receiver.recv())
            .await
            .expect("replacement worker should publish a snapshot")
            .unwrap();
        assert_eq!(starts.load(std::sync::atomic::Ordering::Relaxed), 2);
        assert!(matches!(update, StatusEventUpdate::Snapshot(_)));

        drop(subscriber);
        assert!(hub.stop_if_inactive());
        assert_eq!(hub.lifecycle_for_test(), (0, false));
    }

    #[tokio::test]
    async fn resource_alert_events_emit_open_and_recovery_once() {
        let alert = super::super::AlertItem {
            alert_type: "resource_threshold".to_string(),
            membership_key: String::new(),
            user_id: String::new(),
            endpoint_id: String::new(),
            owner_node_id: "node-a".to_string(),
            quota_banned: false,
            quota_banned_at: None,
            message: "resource threshold".to_string(),
            action_hint: "inspect node resources".to_string(),
            node_id: Some("node-a".to_string()),
            resource_node_id: Some("node-a".to_string()),
            scope: Some("domain".to_string()),
            metric: Some("cpu_busy_percent".to_string()),
            severity: Some("warning".to_string()),
            opened_at: Some("2026-09-01T00:00:00Z".to_string()),
            latest_bucket_start_unix_seconds: Some(60),
        };
        let mut previous = None;
        let opened = resource_alert_events(
            &mut previous,
            &super::super::AlertsResponse {
                partial: false,
                unreachable_nodes: Vec::new(),
                items: vec![alert.clone()],
            },
        );
        assert!(matches!(
            opened.as_slice(),
            [StatusEventUpdate::ResourceAlert {
                event_type: "resource_alert_opened",
                ..
            }]
        ));
        let repeated = resource_alert_events(
            &mut previous,
            &super::super::AlertsResponse {
                partial: false,
                unreachable_nodes: Vec::new(),
                items: vec![alert],
            },
        );
        assert!(repeated.is_empty());
        let recovered = resource_alert_events(
            &mut previous,
            &super::super::AlertsResponse {
                partial: false,
                unreachable_nodes: Vec::new(),
                items: Vec::new(),
            },
        );
        assert!(matches!(
            recovered.as_slice(),
            [StatusEventUpdate::ResourceAlert {
                event_type: "resource_alert_recovered",
                ..
            }]
        ));
    }

    #[test]
    fn an_exited_old_worker_generation_does_not_stop_its_replacement() {
        let hub = StatusEventsHub::new();
        let first_guard = Arc::new(StdMutex::new(None));
        let first_guard_slot = first_guard.clone();
        let first = hub.subscribe(move |worker_guard| {
            *first_guard_slot.lock().unwrap() = Some(worker_guard);
        });

        drop(first);
        assert!(hub.stop_if_inactive());

        let replacement_guard = Arc::new(StdMutex::new(None));
        let replacement_guard_slot = replacement_guard.clone();
        let replacement = hub.subscribe(move |worker_guard| {
            *replacement_guard_slot.lock().unwrap() = Some(worker_guard);
        });

        drop(first_guard.lock().unwrap().take());
        assert_eq!(hub.lifecycle_for_test(), (1, true));

        drop(replacement);
        assert!(hub.stop_if_inactive());
        drop(replacement_guard.lock().unwrap().take());
        assert_eq!(hub.lifecycle_for_test(), (0, false));
    }

    #[test]
    fn replays_the_latest_update_to_a_late_subscriber() {
        let hub = StatusEventsHub::new();
        let _first = hub.subscribe(discard_worker);
        hub.publish_update_if_active(StatusEventUpdate::Snapshot(
            "{\"health\":\"ok\"}".to_string(),
        ));

        let late = hub.subscribe(discard_worker);
        assert!(matches!(
            late.replay,
            Some(StatusEventUpdate::Snapshot(ref snapshot)) if snapshot == "{\"health\":\"ok\"}"
        ));
    }

    #[tokio::test]
    async fn replays_snapshot_errors_and_refreshes_after_a_full_disconnect() {
        let hub = StatusEventsHub::new();
        let first = hub.subscribe(discard_worker);
        let mut fingerprint = Some("initial".to_string());
        hub.publish_error("runtime unavailable".to_string(), &mut fingerprint);

        let late = hub.subscribe(discard_worker);
        assert!(matches!(
            late.replay,
            Some(StatusEventUpdate::SnapshotError(ref error))
                if error.message == "runtime unavailable"
        ));

        drop(first);
        drop(late);
        let reconnect = hub.subscribe(discard_worker);
        assert!(reconnect.replay.is_none());
        let mut stream = Box::pin(super::stream_events(VecDeque::new(), reconnect));
        assert!(hub.publish_snapshot_if_active(
            StatusEventUpdate::Snapshot("{\"health\":\"ok\"}".to_string()),
            "initial",
            &fingerprint,
        ));
        assert_eq!(
            event_text(stream.next().await.unwrap().unwrap()).await,
            "event: snapshot\ndata: {\"health\":\"ok\"}\n\n"
        );
    }

    #[tokio::test]
    async fn keeps_a_refresh_pending_when_a_snapshot_finishes_after_disconnect() {
        let hub = StatusEventsHub::new();
        let first = hub.subscribe(discard_worker);
        drop(first);
        let fingerprint = Some("unchanged".to_string());

        assert!(!hub.publish_snapshot_if_active(
            StatusEventUpdate::Snapshot("{\"health\":\"ok\"}".to_string()),
            "unchanged",
            &fingerprint,
        ));

        let mut reconnect = hub.subscribe(discard_worker);
        assert!(hub.publish_snapshot_if_active(
            StatusEventUpdate::Snapshot("{\"health\":\"ok\"}".to_string()),
            "unchanged",
            &fingerprint,
        ));
        assert!(matches!(
            reconnect.receiver.recv().await.unwrap(),
            StatusEventUpdate::Snapshot(ref snapshot) if snapshot == "{\"health\":\"ok\"}"
        ));
    }

    #[tokio::test]
    async fn publishes_a_recovered_snapshot_after_an_error() {
        let hub = StatusEventsHub::new();
        let mut subscriber = hub.subscribe(discard_worker);
        let mut fingerprint = Some("unchanged".to_string());

        hub.publish_error("runtime unavailable".to_string(), &mut fingerprint);
        assert!(fingerprint.is_none());
        assert!(matches!(
            subscriber.receiver.recv().await.unwrap(),
            StatusEventUpdate::SnapshotError(_)
        ));
        assert!(hub.publish_snapshot_if_active(
            StatusEventUpdate::Snapshot("{\"health\":\"ok\"}".to_string()),
            "unchanged",
            &fingerprint,
        ));
        assert!(matches!(
            subscriber.receiver.recv().await.unwrap(),
            StatusEventUpdate::Snapshot(ref snapshot) if snapshot == "{\"health\":\"ok\"}"
        ));
    }

    #[tokio::test]
    async fn recovers_the_latest_update_after_a_receiver_lags() {
        let hub = StatusEventsHub::new();
        let mut subscriber = hub.subscribe(discard_worker);
        for index in 0..33 {
            hub.publish_update_if_active(StatusEventUpdate::Snapshot(format!(
                "{{\"revision\":{index}}}"
            )));
        }

        assert!(matches!(
            subscriber.receiver.recv().await,
            Err(broadcast::error::RecvError::Lagged(_))
        ));
        assert!(matches!(
            subscriber.recover_after_lag(),
            Some(StatusEventUpdate::Snapshot(ref snapshot)) if snapshot == "{\"revision\":32}"
        ));
    }

    #[tokio::test]
    async fn status_event_stream_delivers_recovery_and_lag_replay() {
        let hub = StatusEventsHub::new();
        let subscription = hub.subscribe(discard_worker);
        let mut stream = Box::pin(super::stream_events(
            std::collections::VecDeque::new(),
            subscription,
        ));
        let mut fingerprint = Some("unchanged".to_string());

        hub.publish_error("runtime unavailable".to_string(), &mut fingerprint);
        assert_eq!(
            event_text(stream.next().await.unwrap().unwrap()).await,
            "event: snapshot_error\ndata: {\"message\":\"runtime unavailable\"}\n\n"
        );
        assert!(hub.publish_snapshot_if_active(
            StatusEventUpdate::Snapshot("{\"health\":\"ok\"}".to_string()),
            "unchanged",
            &fingerprint,
        ));
        assert_eq!(
            event_text(stream.next().await.unwrap().unwrap()).await,
            "event: snapshot\ndata: {\"health\":\"ok\"}\n\n"
        );

        for index in 0..33 {
            hub.publish_update_if_active(StatusEventUpdate::Snapshot(format!(
                "{{\"revision\":{index}}}"
            )));
        }
        assert_eq!(
            event_text(stream.next().await.unwrap().unwrap()).await,
            "event: snapshot\ndata: {\"revision\":32}\n\n"
        );

        drop(stream);
        assert!(hub.stop_if_inactive());
        assert_eq!(hub.lifecycle_for_test(), (0, false));
    }
}
