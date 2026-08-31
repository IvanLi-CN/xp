use std::collections::BTreeMap;

use axum::{
    Json,
    extract::{Extension, Path},
};

use crate::{
    state::history_repository::query::Completeness,
    uptime_monitor::{
        Observation, ServiceMonitor, UPTIME_HISTORY_SCHEMA, UptimeHistoryPayload, status_is_stale,
    },
};

use super::history::{latest_complete_slot_status, status_for};
use super::{ApiError, AppState, HistoryQuality, ObserverStatus, ServiceMonitorStatusResponse};

pub(super) async fn admin_get_service_monitor_status(
    Extension(state): Extension<AppState>,
    Path(monitor_id): Path<String>,
) -> Result<Json<ServiceMonitorStatusResponse>, ApiError> {
    let monitor = super::monitor_for(&state, &monitor_id).await?;
    let now = super::now_unix_seconds();
    let capture = state
        .uptime
        .capture_state()
        .await
        .map_err(super::storage_error)?;
    let observer_node_ids = super::observer_node_ids(&state, &monitor).await;
    let (mut quality, latest_by_observer, recent_observations) =
        latest_observations_for_status(&state, &monitor, now).await?;
    let observers = observer_node_ids
        .into_iter()
        .map(|node_id| {
            let latest = latest_by_observer.get(&node_id).cloned();
            let stale = status_is_stale(
                latest
                    .as_ref()
                    .map(|observation| observation.observed_at_unix_seconds),
                now,
                monitor.interval_seconds,
            );
            ObserverStatus {
                state: status_for(latest.clone().into_iter().collect(), capture, stale),
                latest,
                icmp_supported: (node_id == state.cluster.node_id)
                    .then(crate::uptime_runtime::icmp_supported),
                node_id,
            }
        })
        .collect::<Vec<_>>();
    let latest = observers
        .iter()
        .filter_map(|observer| observer.latest.as_ref())
        .max_by_key(|observation| {
            (
                observation.observed_at_unix_seconds,
                observation.slot_unix_seconds,
            )
        })
        .cloned();
    let stale = status_is_stale(
        latest
            .as_ref()
            .map(|observation| observation.observed_at_unix_seconds),
        now,
        monitor.interval_seconds,
    );
    let (status, latest_slot_complete) =
        latest_complete_slot_status(recent_observations, capture, now, monitor.interval_seconds);
    if !latest_slot_complete {
        quality = HistoryQuality::Partial;
    }
    Ok(Json(ServiceMonitorStatusResponse {
        freshness_seconds: latest
            .as_ref()
            .map(|observation| now.saturating_sub(observation.observed_at_unix_seconds)),
        stale,
        status,
        monitor_id,
        capture,
        quality,
        observers,
    }))
}

async fn latest_observations_for_status(
    state: &AppState,
    monitor: &ServiceMonitor,
    now: u64,
) -> Result<
    (
        HistoryQuality,
        BTreeMap<String, Observation>,
        Vec<Observation>,
    ),
    ApiError,
> {
    let mut latest_by_observer = BTreeMap::new();
    let mut observations_by_key = BTreeMap::new();
    let mut quality = HistoryQuality::LocalOnly;
    let start = now.saturating_sub(u64::from(monitor.interval_seconds).saturating_mul(2));
    if let Ok(repository) = super::super::history_repository::query_service_monitor_history(
        state,
        &monitor.monitor_id,
        start,
        now,
    )
    .await
    {
        quality = history_quality(repository.plan().completeness());
        if repository.records_truncated() {
            quality = HistoryQuality::Partial;
        }
        if repository.plan().completeness() != Completeness::LocalOnly {
            for record in repository.records() {
                if record.schema_id() != UPTIME_HISTORY_SCHEMA {
                    continue;
                }
                let Ok(payload) = serde_json::from_slice::<UptimeHistoryPayload>(record.payload())
                else {
                    continue;
                };
                if payload.ad_hoc {
                    continue;
                }
                if let Some(observation) = payload.latest_observation {
                    insert_status_observation(
                        &mut latest_by_observer,
                        &mut observations_by_key,
                        &monitor.monitor_id,
                        observation,
                    );
                }
            }
        }
    }
    for observation in state
        .uptime
        .observations(
            &monitor.monitor_id,
            start,
            now,
            super::MAX_HISTORY_POINTS.saturating_mul(32),
        )
        .await
        .map_err(super::storage_error)?
    {
        insert_status_observation(
            &mut latest_by_observer,
            &mut observations_by_key,
            &monitor.monitor_id,
            observation,
        );
    }
    Ok((
        quality,
        latest_by_observer,
        observations_by_key.into_values().collect(),
    ))
}

fn insert_status_observation(
    latest_by_observer: &mut BTreeMap<String, Observation>,
    observations_by_key: &mut BTreeMap<(String, u64, u64), Observation>,
    monitor_id: &str,
    observation: Observation,
) {
    if observation.monitor_id != monitor_id || observation.ad_hoc {
        return;
    }
    insert_latest_observation(latest_by_observer, observation.clone());
    let key = (
        observation.observer_node_id.clone(),
        observation.revision,
        observation.slot_unix_seconds,
    );
    if observations_by_key.get(&key).is_none_or(|current| {
        observation.observed_at_unix_seconds >= current.observed_at_unix_seconds
    }) {
        observations_by_key.insert(key, observation);
    }
}

pub(super) fn insert_latest_observation(
    latest_by_observer: &mut BTreeMap<String, Observation>,
    observation: Observation,
) {
    let should_replace = latest_by_observer
        .get(&observation.observer_node_id)
        .is_none_or(|current| {
            (
                observation.observed_at_unix_seconds,
                observation.slot_unix_seconds,
            ) >= (current.observed_at_unix_seconds, current.slot_unix_seconds)
        });
    if should_replace {
        latest_by_observer.insert(observation.observer_node_id.clone(), observation);
    }
}

pub(super) fn history_quality(completeness: Completeness) -> HistoryQuality {
    match completeness {
        Completeness::Complete => HistoryQuality::Complete,
        Completeness::Partial => HistoryQuality::Partial,
        Completeness::LocalOnly => HistoryQuality::LocalOnly,
    }
}
