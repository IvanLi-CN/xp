use std::sync::Arc;

use tokio::{
    sync::Semaphore,
    time::{Duration, MissedTickBehavior},
};

use crate::{
    http::AppState,
    uptime_monitor::{MonitorLifecycle, ServiceMonitor, slot_for},
};

const SCHEDULED_CONCURRENCY: usize = 32;

/// Runs only exact UTC slots. Delayed ticks and restart gaps intentionally do not cause catch-up.
pub(crate) fn spawn_uptime_worker(state: AppState) {
    tokio::spawn(async move {
        let permits = Arc::new(Semaphore::new(SCHEDULED_CONCURRENCY));
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            let now = u64::try_from(chrono::Utc::now().timestamp()).unwrap_or_default();
            if !repository_capture_ready(&state).await {
                continue;
            }
            let Ok(capture_state) = state.uptime.capture_state().await else {
                tracing::debug!("uptime capture state could not be read");
                continue;
            };
            if capture_state.suspended {
                continue;
            }
            let monitors = {
                let store = state.store.lock().await;
                store.list_service_monitors()
            };
            for monitor in monitors {
                if !is_scheduled_on_local_node(&monitor, &state.cluster.node_id, now) {
                    continue;
                }
                let Ok(permit) = permits.clone().try_acquire_owned() else {
                    tracing::debug!(
                        monitor_id = %monitor.monitor_id,
                        "uptime scheduled concurrency is exhausted"
                    );
                    break;
                };
                let uptime = state.uptime.clone();
                let observer_node_id = state.cluster.node_id.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let observation = uptime.run(&monitor, observer_node_id, now, false).await;
                    if let Err(error) = uptime.record(observation).await {
                        tracing::debug!(error = %error, "persist uptime observation failed");
                    }
                });
            }
        }
    });
}

async fn repository_capture_ready(state: &AppState) -> bool {
    let store = state.store.lock().await;
    store
        .state()
        .repository_membership
        .as_ref()
        .is_some_and(|membership| membership.ready_members().next().is_some())
}

pub(super) fn is_scheduled_on_local_node(
    monitor: &ServiceMonitor,
    local_node_id: &str,
    now_unix_seconds: u64,
) -> bool {
    monitor.lifecycle == MonitorLifecycle::Active
        && monitor.revision_effective_at_unix_seconds <= now_unix_seconds
        && monitor
            .observer_node_ids
            .as_ref()
            .is_none_or(|node_ids| node_ids.iter().any(|node_id| node_id == local_node_id))
        && slot_for(now_unix_seconds, monitor.interval_seconds) == now_unix_seconds
}
