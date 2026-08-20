use std::{
    collections::VecDeque,
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
}

impl StatusEventUpdate {
    pub(super) fn into_sse_event(self) -> Event {
        match self {
            Self::Snapshot(snapshot) => Event::default().event("snapshot").data(snapshot),
            Self::SnapshotError(error) => sse_json_event("snapshot_error", &error),
        }
    }
}

#[derive(Default)]
struct StatusEventsLifecycle {
    subscribers: usize,
    worker_running: bool,
    latest_update: Option<StatusEventUpdate>,
    refresh_requested: bool,
}

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
        F: FnOnce(),
    {
        let (receiver, replay, should_start_worker, wake_worker) = {
            let mut lifecycle = self
                .lifecycle
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let receiver = self.sender.subscribe();
            lifecycle.subscribers += 1;
            let should_start_worker = if lifecycle.worker_running {
                false
            } else {
                lifecycle.worker_running = true;
                true
            };
            (
                receiver,
                lifecycle.latest_update.clone(),
                should_start_worker,
                !should_start_worker && lifecycle.refresh_requested,
            )
        };
        if should_start_worker {
            start_worker();
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

    #[cfg(test)]
    fn lifecycle_for_test(&self) -> (usize, bool) {
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (lifecycle.subscribers, lifecycle.worker_running)
    }

    pub(super) async fn run(self, state: AppState) {
        self.run_with_snapshot_builder(move || {
            let state = state.clone();
            async move {
                build_admin_status_snapshot(&state)
                    .await
                    .map_err(|error| error.message)
            }
        })
        .await;
    }

    async fn run_with_snapshot_builder<F, Fut>(self, mut build_snapshot: F)
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<super::AdminStatusSnapshot, String>>,
    {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        let mut last_snapshot_fingerprint: Option<String> = None;

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if self.stop_if_inactive() {
                        return;
                    }
                    self.publish_built_snapshot(
                        build_snapshot().await,
                        &mut last_snapshot_fingerprint,
                    );
                }
                _ = self.shutdown.notified() => {
                    if self.stop_if_inactive() {
                        return;
                    }
                    self.publish_built_snapshot(
                        build_snapshot().await,
                        &mut last_snapshot_fingerprint,
                    );
                }
            }
        }
    }

    fn publish_built_snapshot(
        &self,
        result: Result<super::AdminStatusSnapshot, String>,
        last_snapshot_fingerprint: &mut Option<String>,
    ) {
        match result {
            Ok(snapshot) => self.publish_serialized_snapshot(snapshot, last_snapshot_fingerprint),
            Err(message) => self.publish_error(message, last_snapshot_fingerprint),
        }
    }

    fn publish_serialized_snapshot(
        &self,
        snapshot: super::AdminStatusSnapshot,
        last_snapshot_fingerprint: &mut Option<String>,
    ) {
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
        let mut first = hub.subscribe(move || {
            first_starts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
        let second_starts = starts.clone();
        let mut second = hub.subscribe(move || {
            second_starts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
        let mut first = hub.subscribe(move || {
            worker_starts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            tokio::spawn(async move {
                worker_hub
                    .run_with_snapshot_builder(move || {
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
        let mut second = hub.subscribe(move || {
            second_starts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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

    #[test]
    fn replays_the_latest_update_to_a_late_subscriber() {
        let hub = StatusEventsHub::new();
        let _first = hub.subscribe(|| {});
        hub.publish_update_if_active(StatusEventUpdate::Snapshot(
            "{\"health\":\"ok\"}".to_string(),
        ));

        let late = hub.subscribe(|| {});
        assert!(matches!(
            late.replay,
            Some(StatusEventUpdate::Snapshot(ref snapshot)) if snapshot == "{\"health\":\"ok\"}"
        ));
    }

    #[tokio::test]
    async fn replays_snapshot_errors_and_refreshes_after_a_full_disconnect() {
        let hub = StatusEventsHub::new();
        let first = hub.subscribe(|| {});
        let mut fingerprint = Some("initial".to_string());
        hub.publish_error("runtime unavailable".to_string(), &mut fingerprint);

        let late = hub.subscribe(|| {});
        assert!(matches!(
            late.replay,
            Some(StatusEventUpdate::SnapshotError(ref error))
                if error.message == "runtime unavailable"
        ));

        drop(first);
        drop(late);
        let reconnect = hub.subscribe(|| {});
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
        let first = hub.subscribe(|| {});
        drop(first);
        let fingerprint = Some("unchanged".to_string());

        assert!(!hub.publish_snapshot_if_active(
            StatusEventUpdate::Snapshot("{\"health\":\"ok\"}".to_string()),
            "unchanged",
            &fingerprint,
        ));

        let mut reconnect = hub.subscribe(|| {});
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
        let mut subscriber = hub.subscribe(|| {});
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
        let mut subscriber = hub.subscribe(|| {});
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
        let subscription = hub.subscribe(|| {});
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
