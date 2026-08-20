use std::sync::{Arc, Mutex as StdMutex};

use axum::response::sse::Event;
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
}

#[derive(Clone)]
pub(super) struct StatusEventsHub {
    sender: broadcast::Sender<StatusEventUpdate>,
    lifecycle: Arc<StdMutex<StatusEventsLifecycle>>,
    shutdown: Arc<Notify>,
}

pub(super) struct StatusEventsSubscription {
    pub(super) receiver: broadcast::Receiver<StatusEventUpdate>,
    hub: StatusEventsHub,
}

impl Drop for StatusEventsSubscription {
    fn drop(&mut self) {
        self.hub.unsubscribe();
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
        let receiver = self.sender.subscribe();
        let should_start_worker = {
            let mut lifecycle = self
                .lifecycle
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            lifecycle.subscribers += 1;
            if lifecycle.worker_running {
                false
            } else {
                lifecycle.worker_running = true;
                true
            }
        };
        if should_start_worker {
            start_worker();
        }
        StatusEventsSubscription {
            receiver,
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
            lifecycle.subscribers == 0
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
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        let mut last_snapshot_fingerprint: Option<String> = None;

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if self.stop_if_inactive() {
                        return;
                    }
                    self.publish_snapshot(&state, &mut last_snapshot_fingerprint).await;
                }
                _ = self.shutdown.notified() => {
                    if self.stop_if_inactive() {
                        return;
                    }
                }
            }
        }
    }

    async fn publish_snapshot(
        &self,
        state: &AppState,
        last_snapshot_fingerprint: &mut Option<String>,
    ) {
        match build_admin_status_snapshot(state).await {
            Ok(snapshot) => self.publish_serialized_snapshot(snapshot, last_snapshot_fingerprint),
            Err(error) => self.publish_error(error.message),
        }
    }

    fn publish_serialized_snapshot(
        &self,
        snapshot: super::AdminStatusSnapshot,
        last_snapshot_fingerprint: &mut Option<String>,
    ) {
        let snapshot_json = match serde_json::to_string(&snapshot) {
            Ok(snapshot_json) => snapshot_json,
            Err(error) => return self.publish_error(format!("serialize status snapshot: {error}")),
        };
        match admin_status_snapshot_fingerprint(&snapshot) {
            Ok(fingerprint) if last_snapshot_fingerprint.as_ref() != Some(&fingerprint) => {
                let _ = self.sender.send(StatusEventUpdate::Snapshot(snapshot_json));
                *last_snapshot_fingerprint = Some(fingerprint);
            }
            Ok(_) => {}
            Err(error) => self.publish_error(format!("serialize status snapshot: {error}")),
        }
    }

    fn publish_error(&self, message: String) {
        let _ = self
            .sender
            .send(StatusEventUpdate::SnapshotError(AdminStatusSnapshotError {
                message,
            }));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::AtomicUsize};

    use super::*;

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

        assert!(
            hub.sender
                .send(StatusEventUpdate::Snapshot(
                    "{\"health\":\"ok\"}".to_string(),
                ))
                .is_ok()
        );
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
}
