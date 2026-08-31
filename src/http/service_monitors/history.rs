use std::collections::{BTreeMap, BTreeSet};

use axum::{
    Json,
    extract::{Extension, Path, Query},
};

use super::{
    AppState, CaptureState, CurrentStatus, HistoryGap, HistoryPoint, HistoryQuality, HistoryQuery,
    MAX_HISTORY_POINTS, Observation, ObservationRollup, RECENT_SUMMARY_BUCKET_COUNT,
    RECENT_SUMMARY_BUCKET_SECONDS, RecentHistorySummary, ServiceMonitor,
    ServiceMonitorHistoryResponse,
};

use crate::{
    http::ApiError,
    state::history_repository::query::Completeness,
    uptime_monitor::{
        ExpectedSlotRange, UPTIME_HISTORY_SCHEMA, UptimeHistoryPayload, current_status,
        normalized_observer_set,
    },
};

pub(super) async fn admin_get_service_monitor_history(
    Extension(state): Extension<AppState>,
    Path(monitor_id): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<ServiceMonitorHistoryResponse>, ApiError> {
    let _monitor = super::monitor_for(&state, &monitor_id).await?;
    let now = super::now_unix_seconds();
    let end = query.to.unwrap_or(now).min(now);
    let start = query
        .from
        .unwrap_or_else(|| end.saturating_sub(24 * 60 * 60));
    if start > end {
        return Err(ApiError::invalid_request(
            "history from must not be after to",
        ));
    }
    let requested_limit = query.limit.unwrap_or(MAX_HISTORY_POINTS);
    let limit = requested_limit.clamp(1, MAX_HISTORY_POINTS);
    let (resolution, seconds) = select_resolution(query.resolution.as_deref(), start, end, limit)?;
    let repository = super::super::history_repository::query_service_monitor_history(
        &state,
        &monitor_id,
        start,
        end,
    )
    .await?;
    let mut quality = super::status::history_quality(repository.plan().completeness());
    let repository_selected = repository.plan().completeness() != Completeness::LocalOnly;
    let repository_truncated = repository.records_truncated();
    let mut buckets = BTreeMap::<u64, ObservationRollup>::new();
    let mut expected_observer_counts_by_slot = BTreeMap::<u64, u32>::new();
    let mut expected_slot_ranges = BTreeSet::<ExpectedSlotRange>::new();
    let mut missing_expected_snapshot = false;
    if repository_selected {
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
            if payload.expected_observer_counts_by_slot.is_empty()
                && payload.observer_sets_by_slot.is_empty()
                && payload.rollup.executed > 0
            {
                missing_expected_snapshot = true;
            }
            collect_expected_slots(
                &payload,
                start,
                end,
                &mut expected_observer_counts_by_slot,
                &mut expected_slot_ranges,
            );
            if query
                .observer_id
                .as_deref()
                .is_some_and(|observer_id| observer_id != payload.observer_node_id)
            {
                continue;
            }
            let observed_at = payload
                .bucket_start_unix_seconds
                .unwrap_or_else(|| record.observed_at_unix_seconds());
            let bucket_start = observed_at / seconds * seconds;
            buckets
                .entry(bucket_start)
                .or_default()
                .merge(&payload.rollup);
        }
    } else {
        let observations = state
            .uptime
            .observations(
                &monitor_id,
                start,
                end,
                MAX_HISTORY_POINTS.saturating_mul(32),
            )
            .await
            .map_err(super::storage_error)?;
        let capture_gaps = state
            .uptime
            .capture_gaps(
                &monitor_id,
                start,
                end,
                MAX_HISTORY_POINTS.saturating_mul(32),
            )
            .await
            .map_err(super::storage_error)?;
        expected_slot_ranges.extend(capture_gaps.into_iter().map(|gap| gap.range));
        for observation in observations.into_iter().filter(|observation| {
            !observation.ad_hoc
                && query
                    .observer_id
                    .as_deref()
                    .is_none_or(|observer_id| observer_id == observation.observer_node_id)
        }) {
            let expected_observer_count = u32::try_from(
                observation
                    .observer_set_node_ids
                    .iter()
                    .filter(|node_id| !node_id.trim().is_empty())
                    .count(),
            )
            .unwrap_or(u32::MAX)
            .max(observation.expected_observer_count)
            .max(1);
            expected_observer_counts_by_slot
                .entry(observation.slot_unix_seconds)
                .and_modify(|current| *current = (*current).max(expected_observer_count))
                .or_insert(expected_observer_count);
            let bucket_start = observation.observed_at_unix_seconds / seconds * seconds;
            let rollup = buckets.entry(bucket_start).or_default();
            rollup.record(&observation);
        }
    }
    let first_bucket = start / seconds * seconds;
    let last_bucket = end / seconds * seconds;
    let bucket_count = last_bucket
        .saturating_sub(first_bucket)
        .saturating_div(seconds)
        .saturating_add(1);
    let visible_bucket_count = bucket_count.min(u64::try_from(limit).unwrap_or(u64::MAX));
    let visible_first_bucket = last_bucket.saturating_sub(
        visible_bucket_count
            .saturating_sub(1)
            .saturating_mul(seconds),
    );
    let mut bucket_start = visible_first_bucket;
    while bucket_start <= last_bucket {
        buckets.entry(bucket_start).or_default();
        let Some(next_bucket) = bucket_start.checked_add(seconds) else {
            break;
        };
        bucket_start = next_bucket;
    }
    let mut points = buckets
        .into_iter()
        .map(|(bucket_start, mut rollup)| {
            let bucket_end = bucket_start
                .saturating_add(seconds.saturating_sub(1))
                .min(end);
            let expected = expected_observations_in_bucket(
                &expected_observer_counts_by_slot,
                &expected_slot_ranges,
                bucket_start.max(start),
                bucket_end,
                query.observer_id.is_some(),
            );
            if expected == 0 && rollup.executed > 0 {
                missing_expected_snapshot = true;
            }
            rollup.expected = expected.max(rollup.executed);
            HistoryPoint {
                start_unix_seconds: bucket_start,
                end_unix_seconds: bucket_end,
                availability_percent: rollup.availability_percent(),
                coverage_percent: rollup.coverage_percent(),
                rollup,
            }
        })
        .collect::<Vec<_>>();
    let truncated = repository_truncated || bucket_count > u64::try_from(limit).unwrap_or(u64::MAX);
    if truncated {
        points.drain(..points.len().saturating_sub(limit));
    }
    let expected = points
        .iter()
        .map(|point| point.rollup.expected)
        .sum::<u64>();
    let executed = points
        .iter()
        .map(|point| point.rollup.executed)
        .sum::<u64>();
    let gaps: Vec<HistoryGap> = points
        .iter()
        .filter(|point| point.rollup.executed < point.rollup.expected)
        .map(|point| HistoryGap {
            start_unix_seconds: point.start_unix_seconds,
            end_unix_seconds: point.end_unix_seconds,
            expected: point.rollup.expected,
            executed: point.rollup.executed,
        })
        .collect();
    if truncated || missing_expected_snapshot || !gaps.is_empty() {
        quality = HistoryQuality::Partial;
    }
    let latest = state
        .uptime
        .latest(&monitor_id)
        .await
        .map_err(super::storage_error)?;
    Ok(Json(ServiceMonitorHistoryResponse {
        monitor_id,
        resolution,
        coverage_percent: (expected > 0).then(|| executed as f64 * 100.0 / expected as f64),
        watermark_unix_seconds: latest
            .as_ref()
            .map(|observation| observation.observed_at_unix_seconds),
        freshness_seconds: latest
            .as_ref()
            .map(|observation| now.saturating_sub(observation.observed_at_unix_seconds)),
        quality,
        points,
        truncated,
        gaps,
        skew_seconds: 0,
    }))
}

fn collect_expected_slots(
    payload: &UptimeHistoryPayload,
    start: u64,
    end: u64,
    expected_observer_counts_by_slot: &mut BTreeMap<u64, u32>,
    expected_slot_ranges: &mut BTreeSet<ExpectedSlotRange>,
) {
    for (slot, observer_set_node_ids) in &payload.observer_sets_by_slot {
        if (start..=end).contains(slot) {
            let expected_observer_count = u32::try_from(observer_set_node_ids.len())
                .unwrap_or(u32::MAX)
                .max(1);
            expected_observer_counts_by_slot
                .entry(*slot)
                .and_modify(|current| *current = (*current).max(expected_observer_count))
                .or_insert(expected_observer_count);
        }
    }
    for (slot, expected_observer_count) in &payload.expected_observer_counts_by_slot {
        if (start..=end).contains(slot) {
            expected_observer_counts_by_slot
                .entry(*slot)
                .and_modify(|current| *current = (*current).max(*expected_observer_count))
                .or_insert(*expected_observer_count);
        }
    }
    expected_slot_ranges.extend(
        payload
            .expected_slot_ranges
            .iter()
            .filter(|range| range.slot_count_in(start, end) > 0)
            .cloned(),
    );
}

pub(super) fn expected_observations_in_bucket(
    expected_observer_counts_by_slot: &BTreeMap<u64, u32>,
    expected_slot_ranges: &BTreeSet<ExpectedSlotRange>,
    start: u64,
    end: u64,
    observer_filter: bool,
) -> u64 {
    let expected_from_ranges = expected_slot_ranges
        .iter()
        .map(|range| {
            let observer_count = if observer_filter {
                1
            } else {
                u64::from(range.observer_count())
            };
            range
                .slot_count_in(start, end)
                .saturating_mul(observer_count)
        })
        .sum::<u64>();
    let expected_from_slots = expected_observer_counts_by_slot
        .range(start..=end)
        .filter(|(slot, _)| {
            !expected_slot_ranges
                .iter()
                .any(|range| range.slot_count_in(**slot, **slot) > 0)
        })
        .map(|(_, expected_observer_count)| {
            if observer_filter {
                1
            } else {
                u64::from((*expected_observer_count).max(1))
            }
        })
        .sum::<u64>();
    expected_from_ranges.saturating_add(expected_from_slots)
}

pub(super) fn recent_history_summary(
    monitor: &ServiceMonitor,
    capture: CaptureState,
    now: u64,
    latest: Option<&Observation>,
    observations: Vec<Observation>,
) -> RecentHistorySummary {
    let window_seconds =
        RECENT_SUMMARY_BUCKET_SECONDS.saturating_mul(RECENT_SUMMARY_BUCKET_COUNT as u64);
    let start = now.saturating_sub(window_seconds);
    let first_bucket = now
        .saturating_div(RECENT_SUMMARY_BUCKET_SECONDS)
        .saturating_sub((RECENT_SUMMARY_BUCKET_COUNT - 1) as u64)
        .saturating_mul(RECENT_SUMMARY_BUCKET_SECONDS);
    let mut buckets = vec![ObservationRollup::default(); RECENT_SUMMARY_BUCKET_COUNT];
    for observation in observations
        .into_iter()
        .filter(|observation| !observation.ad_hoc)
    {
        let bucket = observation.observed_at_unix_seconds / RECENT_SUMMARY_BUCKET_SECONDS
            * RECENT_SUMMARY_BUCKET_SECONDS;
        let Some(index) = bucket
            .checked_sub(first_bucket)
            .and_then(|offset| usize::try_from(offset / RECENT_SUMMARY_BUCKET_SECONDS).ok())
            .filter(|index| *index < RECENT_SUMMARY_BUCKET_COUNT)
        else {
            continue;
        };
        buckets[index].record(&observation);
    }
    let mut total = ObservationRollup::default();
    let slots = buckets
        .iter_mut()
        .enumerate()
        .map(|(index, rollup)| {
            let bucket_start = first_bucket
                .saturating_add(RECENT_SUMMARY_BUCKET_SECONDS.saturating_mul(index as u64));
            let bucket_end = bucket_start
                .saturating_add(RECENT_SUMMARY_BUCKET_SECONDS.saturating_sub(1))
                .min(now);
            rollup.record_expected(expected_slots(
                bucket_start.max(start),
                bucket_end,
                monitor.interval_seconds,
            ));
            total.merge(rollup);
            status_for_rollup(rollup, capture)
        })
        .collect();
    RecentHistorySummary {
        availability_percent: total.availability_percent(),
        coverage_percent: total.coverage_percent(),
        expected: total.expected,
        executed: total.executed,
        latest_latency_ms: latest.and_then(|observation| observation.latency_ms),
        latest_observed_at_unix_seconds: latest
            .map(|observation| observation.observed_at_unix_seconds),
        slots,
    }
}

fn status_for_rollup(rollup: &ObservationRollup, capture: CaptureState) -> CurrentStatus {
    if capture.suspended {
        return CurrentStatus::CaptureSuspended;
    }
    let usable = rollup.successes.saturating_add(rollup.failures);
    if usable == 0 {
        return if rollup.suspended > 0 {
            CurrentStatus::CaptureSuspended
        } else {
            CurrentStatus::Unknown
        };
    }
    if rollup.successes == usable {
        CurrentStatus::Up
    } else if rollup.successes == 0 {
        CurrentStatus::Down
    } else {
        CurrentStatus::Degraded
    }
}

pub(super) fn expected_slots(start: u64, end: u64, interval_seconds: u32) -> u64 {
    let interval = u64::from(interval_seconds);
    let first = start.div_ceil(interval).saturating_mul(interval);
    if first <= end {
        end.saturating_sub(first) / interval + 1
    } else {
        0
    }
}

pub(super) fn select_resolution(
    requested: Option<&str>,
    start: u64,
    end: u64,
    limit: usize,
) -> Result<(String, u64), ApiError> {
    let range = end.saturating_sub(start).saturating_add(1);
    let resolution = match requested.unwrap_or("auto") {
        "auto" => {
            if range.div_ceil(60) <= limit as u64 {
                ("1m", 60)
            } else if range.div_ceil(5 * 60) <= limit as u64 {
                ("5m", 5 * 60)
            } else {
                ("1h", 60 * 60)
            }
        }
        "1m" => ("1m", 60),
        "5m" => ("5m", 5 * 60),
        "1h" => ("1h", 60 * 60),
        _ => {
            return Err(ApiError::invalid_request(
                "history resolution must be auto, 1m, 5m, or 1h",
            ));
        }
    };
    Ok((resolution.0.to_owned(), resolution.1))
}

pub(super) fn status_for(
    observations: Vec<Observation>,
    capture: CaptureState,
    stale: bool,
) -> CurrentStatus {
    if capture.suspended {
        CurrentStatus::CaptureSuspended
    } else if stale {
        CurrentStatus::Unknown
    } else {
        current_status(observations)
    }
}

/// A monitor is only currently up/down when every observer in the newest slot snapshot has a
/// fresh result for that exact slot and revision. Mixing independent "latest" results would let
/// a stale success hide a missing observer or an older failure distort the current status.
pub(super) fn latest_complete_slot_status(
    observations: Vec<Observation>,
    capture: CaptureState,
    now_unix_seconds: u64,
    interval_seconds: u32,
) -> (CurrentStatus, bool) {
    if capture.suspended {
        return (CurrentStatus::CaptureSuspended, true);
    }
    let fresh = observations
        .into_iter()
        .filter(|observation| {
            !observation.ad_hoc
                && !super::status_is_stale(
                    Some(observation.observed_at_unix_seconds),
                    now_unix_seconds,
                    interval_seconds,
                )
        })
        .collect::<Vec<_>>();
    let Some((slot_unix_seconds, revision)) = fresh
        .iter()
        .map(|observation| (observation.slot_unix_seconds, observation.revision))
        .max()
    else {
        return (CurrentStatus::Unknown, false);
    };
    let latest_slot = fresh
        .into_iter()
        .filter(|observation| {
            (observation.slot_unix_seconds, observation.revision) == (slot_unix_seconds, revision)
        })
        .collect::<Vec<_>>();
    let Some(observer_set) = latest_slot
        .first()
        .and_then(|observation| normalized_observer_set(&observation.observer_set_node_ids))
    else {
        return (CurrentStatus::Unknown, false);
    };
    let mut observations_by_observer = BTreeMap::new();
    for observation in latest_slot {
        if normalized_observer_set(&observation.observer_set_node_ids).as_ref()
            != Some(&observer_set)
            || !observer_set.contains(&observation.observer_node_id)
        {
            return (CurrentStatus::Unknown, false);
        }
        observations_by_observer.insert(observation.observer_node_id.clone(), observation);
    }
    let complete = observer_set
        .iter()
        .all(|observer_node_id| observations_by_observer.contains_key(observer_node_id));
    if !complete {
        return (CurrentStatus::Unknown, false);
    }
    (current_status(observations_by_observer.into_values()), true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uptime_monitor::ObservationOutcome;

    fn observation() -> Observation {
        Observation {
            monitor_id: "monitor".to_owned(),
            revision: 1,
            observer_node_id: "observer".to_owned(),
            observer_set_node_ids: vec!["observer".to_owned()],
            expected_observer_count: 1,
            slot_unix_seconds: 60,
            observed_at_unix_seconds: 60,
            outcome: ObservationOutcome::Success,
            error: None,
            latency_ms: Some(1),
            status_code: None,
            packet_loss_percent: 0,
            ad_hoc: false,
        }
    }

    #[test]
    fn stale_observations_are_unknown_unless_capture_is_suspended() {
        let capture = CaptureState {
            suspended: false,
            pending_observations: 0,
            pending_bytes: 0,
        };
        assert_eq!(
            status_for(vec![observation()], capture, true),
            CurrentStatus::Unknown
        );
        assert_eq!(
            status_for(
                vec![observation()],
                CaptureState {
                    suspended: true,
                    ..capture
                },
                true,
            ),
            CurrentStatus::CaptureSuspended
        );
    }

    #[test]
    fn latest_status_requires_one_complete_observer_set_slot() {
        let capture = CaptureState {
            suspended: false,
            pending_observations: 0,
            pending_bytes: 0,
        };
        let mut tokyo = observation();
        tokyo.observer_node_id = "tokyo".to_owned();
        tokyo.observer_set_node_ids = vec!["tokyo".to_owned(), "singapore".to_owned()];
        tokyo.slot_unix_seconds = 120;
        tokyo.observed_at_unix_seconds = 120;
        let mut singapore = tokyo.clone();
        singapore.observer_node_id = "singapore".to_owned();
        singapore.slot_unix_seconds = 60;
        singapore.observed_at_unix_seconds = 60;
        singapore.outcome = ObservationOutcome::Failure;

        assert_eq!(
            latest_complete_slot_status(vec![tokyo, singapore], capture, 121, 60),
            (CurrentStatus::Unknown, false)
        );
    }

    #[test]
    fn latest_status_uses_results_from_the_same_complete_slot() {
        let capture = CaptureState {
            suspended: false,
            pending_observations: 0,
            pending_bytes: 0,
        };
        let mut tokyo = observation();
        tokyo.observer_node_id = "tokyo".to_owned();
        tokyo.observer_set_node_ids = vec!["tokyo".to_owned(), "singapore".to_owned()];
        tokyo.slot_unix_seconds = 120;
        tokyo.observed_at_unix_seconds = 120;
        let mut singapore = tokyo.clone();
        singapore.observer_node_id = "singapore".to_owned();
        singapore.outcome = ObservationOutcome::Failure;

        assert_eq!(
            latest_complete_slot_status(vec![tokyo, singapore], capture, 121, 60),
            (CurrentStatus::Degraded, true)
        );
    }

    #[test]
    fn compressed_capture_gap_contributes_expected_slots_without_observations() {
        let range = ExpectedSlotRange {
            start_slot_unix_seconds: 60,
            end_slot_unix_seconds: 180,
            interval_seconds: 60,
            observer_set_node_ids: vec!["tokyo".to_owned(), "singapore".to_owned()],
        };
        let payload = UptimeHistoryPayload::from_capture_gap(
            "monitor".to_owned(),
            1,
            "tokyo".to_owned(),
            range,
        );
        let mut expected_observer_counts_by_slot = BTreeMap::new();
        let mut ranges = BTreeSet::new();
        collect_expected_slots(
            &payload,
            60,
            180,
            &mut expected_observer_counts_by_slot,
            &mut ranges,
        );
        assert!(expected_observer_counts_by_slot.is_empty());
        assert_eq!(ranges.len(), 1);
        assert_eq!(
            expected_observations_in_bucket(&BTreeMap::new(), &ranges, 60, 180, false),
            6
        );
        assert_eq!(
            expected_observations_in_bucket(&BTreeMap::new(), &ranges, 60, 180, true),
            3
        );
    }
}
