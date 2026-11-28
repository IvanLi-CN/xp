use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{ConnectionClass, Point, PointSeries};

pub const TCP_CONNECTION_HISTORY_SCHEMA: &str = "node_tcp_connections.v1";
pub const TCP_CONNECTION_HISTORY_STREAM: &str = "node_tcp_connections-v1";
pub const REPOSITORY_CAPABILITY: &str = "history.node-tcp-connections.v1";
pub const REPOSITORY_RETENTION_DAYS: i64 = 90;
pub const MAX_SUBJECTS_PER_RECORD: usize = 100;

/// Shard 0 of one daily bucket: node total, per-class points, and a bounded subject page.
/// Subject detail beyond the page is omitted rather than silently truncated mid-record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyBucketPayload {
    pub bucket_start_unix_seconds: u64,
    pub bucket_end_unix_seconds: u64,
    pub total: Point,
    pub classes: Vec<ClassPoint>,
    #[serde(default)]
    pub subjects: Vec<SubjectPoint>,
    pub subjects_omitted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassPoint {
    pub class: String,
    pub point: Point,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubjectPoint {
    pub class: String,
    pub subject: String,
    pub point: Point,
}

impl Point {
    pub fn bucket_end_unix_seconds(&self) -> i64 {
        self.bucket_start.timestamp() + 24 * 60 * 60 - 1
    }
}

pub fn daily_record_key(node_id: &str, bucket_start_unix_seconds: u64) -> Vec<u8> {
    format!("node-history:node:{node_id}:daily:{bucket_start_unix_seconds}:0").into_bytes()
}

pub fn daily_payload_from_series(
    series: &PointSeries,
    bucket_start: DateTime<Utc>,
) -> Option<DailyBucketPayload> {
    let total = series
        .total
        .iter()
        .find(|point| point.bucket_start == bucket_start)?
        .clone();
    let classes = series
        .classes
        .iter()
        .filter_map(|(class, points)| {
            points
                .iter()
                .find(|point| point.bucket_start == bucket_start)
                .map(|point| ClassPoint {
                    class: class.as_str().to_string(),
                    point: point.clone(),
                })
        })
        .collect::<Vec<_>>();
    let mut subjects = Vec::new();
    let mut subjects_omitted = false;
    for ((class, subject), points) in &series.subjects {
        let Some(point) = points
            .iter()
            .find(|point| point.bucket_start == bucket_start)
        else {
            continue;
        };
        if subjects.len() >= MAX_SUBJECTS_PER_RECORD {
            subjects_omitted = true;
            break;
        }
        subjects.push(SubjectPoint {
            class: class.as_str().to_string(),
            subject: subject.clone(),
            point: point.clone(),
        });
    }
    Some(DailyBucketPayload {
        bucket_start_unix_seconds: u64::try_from(bucket_start.timestamp()).ok()?,
        bucket_end_unix_seconds: u64::try_from(total.bucket_end_unix_seconds()).ok()?,
        total,
        classes,
        subjects,
        subjects_omitted,
    })
}

/// Rebuilds query series from repository daily payloads. The repository is authoritative for
/// 30d/90d windows, so this path never re-derives totals from subject maxima.
pub fn series_from_daily_payloads(payloads: &[DailyBucketPayload]) -> PointSeries {
    let mut series = PointSeries::default();
    for payload in payloads {
        series.total.push(payload.total.clone());
        for item in &payload.classes {
            let Some(class) = ConnectionClass::parse(&item.class) else {
                continue;
            };
            series
                .classes
                .entry(class)
                .or_default()
                .push(item.point.clone());
        }
        for item in &payload.subjects {
            let Some(class) = ConnectionClass::parse(&item.class) else {
                continue;
            };
            series
                .subjects
                .entry((class, item.subject.clone()))
                .or_default()
                .push(item.point.clone());
        }
    }
    series.total.sort_by_key(|point| point.bucket_start);
    for points in series.classes.values_mut() {
        points.sort_by_key(|point| point.bucket_start);
    }
    for points in series.subjects.values_mut() {
        points.sort_by_key(|point| point.bucket_start);
    }
    series
}

pub fn parse_daily_payload(payload: &[u8]) -> Option<DailyBucketPayload> {
    serde_json::from_slice(payload).ok()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::tcp_connection_history::rollup::{PointSeries, Resolution, aggregate_points};
    use crate::tcp_connection_history::test_support::stored_minute;

    #[test]
    fn daily_payload_binds_bucket_totals_and_classes() {
        let minutes = vec![stored_minute("2026-11-26T10:00:00Z", 3)];
        let start = DateTime::parse_from_rfc3339("2026-11-26T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let end = DateTime::parse_from_rfc3339("2026-11-26T23:59:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let series = aggregate_points(&minutes, Resolution::Daily, start, end);
        let payload = daily_payload_from_series(&series, start).unwrap();
        assert_eq!(payload.total.max, 3);
        let series = series_from_daily_payloads(&[payload.clone()]);
        assert_eq!(series.classes[&ConnectionClass::Inbound][0].max, 3);
        assert!(!payload.subjects_omitted);
        assert_eq!(
            daily_record_key("node-a", payload.bucket_start_unix_seconds),
            format!(
                "node-history:node:node-a:daily:{}:0",
                payload.bucket_start_unix_seconds
            )
            .into_bytes()
        );
    }

    #[test]
    fn subject_page_is_bounded_and_marks_omission() {
        let mut counts = BTreeMap::new();
        for index in 0..(MAX_SUBJECTS_PER_RECORD + 5) {
            counts.insert((ConnectionClass::OutboundOther, index.to_string()), 1_u32);
        }
        let total = counts.values().sum();
        let minutes = vec![
            crate::tcp_connection_history::test_support::stored_minute_with_counts(
                "2026-11-26T10:00:00Z",
                total,
                counts,
            ),
        ];
        let start = DateTime::parse_from_rfc3339("2026-11-26T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let end = DateTime::parse_from_rfc3339("2026-11-26T23:59:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let series: PointSeries = aggregate_points(&minutes, Resolution::Daily, start, end);
        let payload = daily_payload_from_series(&series, start).unwrap();
        assert_eq!(payload.subjects.len(), MAX_SUBJECTS_PER_RECORD);
        assert!(payload.subjects_omitted);
        assert_eq!(payload.total.max as usize, MAX_SUBJECTS_PER_RECORD + 5);
    }
}
