use super::{
    CaptureState, CurrentStatus, Observation, ObservationRollup, RECENT_SUMMARY_BUCKET_COUNT,
    RECENT_SUMMARY_BUCKET_SECONDS, RecentHistorySummary, ServiceMonitor,
};

use crate::{http::ApiError, uptime_monitor::current_status};

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

pub(super) fn status_for(observations: Vec<Observation>, capture: CaptureState) -> CurrentStatus {
    if capture.suspended {
        CurrentStatus::CaptureSuspended
    } else {
        current_status(observations)
    }
}
