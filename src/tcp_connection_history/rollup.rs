use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Timelike as _, Utc};
use serde::{Deserialize, Serialize};

use super::{ConnectionClass, StoredMinute, TcpConnectionHistoryError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Resolution {
    FiveMinute,
    Daily,
}

impl Resolution {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FiveMinute => "5m",
            Self::Daily => "daily",
        }
    }

    pub fn minutes(self) -> i64 {
        match self {
            Self::FiveMinute => 5,
            Self::Daily => 24 * 60,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub bucket_start: DateTime<Utc>,
    pub min: u32,
    pub avg: f64,
    pub max: u32,
    pub samples_expected: u32,
    pub samples_actual: u32,
    pub truncated: bool,
    pub partial: bool,
}

#[derive(Debug, Clone, Default)]
pub struct PointSeries {
    pub total: Vec<Point>,
    pub classes: BTreeMap<ConnectionClass, Vec<Point>>,
    pub subjects: BTreeMap<(ConnectionClass, String), Vec<Point>>,
}

pub fn floor_bucket(at: DateTime<Utc>, resolution: Resolution) -> DateTime<Utc> {
    let minute = at
        .with_second(0)
        .and_then(|dt| dt.with_nanosecond(0))
        .unwrap_or(at);
    let minutes = resolution.minutes();
    let epoch_minute = minute.timestamp().div_euclid(60);
    let bucket_minute = epoch_minute - epoch_minute.rem_euclid(minutes);
    DateTime::from_timestamp(bucket_minute * 60, 0).unwrap_or(minute)
}

fn empty_point(bucket_start: DateTime<Utc>, expected: u32) -> Point {
    Point {
        bucket_start,
        min: 0,
        avg: 0.0,
        max: 0,
        samples_expected: expected,
        samples_actual: 0,
        truncated: false,
        partial: false,
    }
}

fn accumulate(point: &mut Point, value: u32) {
    if point.samples_actual == 0 {
        point.min = value;
        point.max = value;
    } else {
        point.min = point.min.min(value);
        point.max = point.max.max(value);
    }
    point.samples_actual = point.samples_actual.saturating_add(1);
    let samples = f64::from(point.samples_actual);
    point.avg = (point.avg * (samples - 1.0) + f64::from(value)) / samples;
}

fn expected_minutes(
    bucket_start: DateTime<Utc>,
    resolution: Resolution,
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
) -> u32 {
    let bucket_end = bucket_start + Duration::minutes(resolution.minutes());
    let start = bucket_start.max(window_start);
    let end = bucket_end.min(window_end + Duration::minutes(1));
    end.signed_duration_since(start)
        .num_minutes()
        .clamp(0, resolution.minutes()) as u32
}

pub fn aggregate_points(
    minutes: &[StoredMinute],
    resolution: Resolution,
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
) -> PointSeries {
    let mut by_bucket = BTreeMap::<i64, Vec<&StoredMinute>>::new();
    for minute in minutes {
        if minute.minute < window_start || minute.minute > window_end {
            continue;
        }
        by_bucket
            .entry(floor_bucket(minute.minute, resolution).timestamp())
            .or_default()
            .push(minute);
    }

    let mut series = PointSeries::default();
    for (bucket_unix, observed) in by_bucket {
        let Some(bucket_start) = DateTime::from_timestamp(bucket_unix, 0) else {
            continue;
        };
        let expected = expected_minutes(bucket_start, resolution, window_start, window_end);
        let mut total = empty_point(bucket_start, expected);
        let mut classes = BTreeMap::<ConnectionClass, Point>::new();
        let mut subjects = BTreeMap::<(ConnectionClass, String), Point>::new();

        for minute in &observed {
            accumulate(&mut total, minute.total);
            total.truncated |= minute.truncated;
            total.partial |= minute.partial;

            let mut class_totals = BTreeMap::<ConnectionClass, u32>::new();
            for ((class, subject), count) in &minute.counts {
                class_totals
                    .entry(*class)
                    .and_modify(|value| *value = value.saturating_add(*count))
                    .or_insert(*count);
                accumulate(
                    subjects
                        .entry((*class, subject.clone()))
                        .or_insert_with(|| empty_point(bucket_start, expected)),
                    *count,
                );
            }
            for (class, value) in class_totals {
                accumulate(
                    classes
                        .entry(class)
                        .or_insert_with(|| empty_point(bucket_start, expected)),
                    value,
                );
            }
        }

        for point in classes.values_mut().chain(subjects.values_mut()) {
            point.truncated |= total.truncated;
            point.partial |= total.partial;
        }
        series.total.push(total);
        for (class, point) in classes {
            series.classes.entry(class).or_default().push(point);
        }
        for (key, point) in subjects {
            series.subjects.entry(key).or_default().push(point);
        }
    }
    series
}

/// Per-minute series used by the 24h window. Each minute is already an instantaneous sample,
/// so the point carries min = avg = max = total with one observed sample.
pub fn minute_series(minutes: &[StoredMinute]) -> PointSeries {
    let mut series = PointSeries::default();
    for minute in minutes {
        let mut point = empty_point(minute.minute, 1);
        accumulate(&mut point, minute.total);
        point.truncated = minute.truncated;
        point.partial = minute.partial;
        series.total.push(point);

        let mut class_totals = BTreeMap::<ConnectionClass, u32>::new();
        for ((class, subject), count) in &minute.counts {
            class_totals
                .entry(*class)
                .and_modify(|value| *value = value.saturating_add(*count))
                .or_insert(*count);
            let mut subject_point = empty_point(minute.minute, 1);
            accumulate(&mut subject_point, *count);
            series
                .subjects
                .entry((*class, subject.clone()))
                .or_default()
                .push(subject_point);
        }
        for (class, value) in class_totals {
            let mut class_point = empty_point(minute.minute, 1);
            accumulate(&mut class_point, value);
            series.classes.entry(class).or_default().push(class_point);
        }
    }
    series
}

pub fn load_rollups(
    conn: &rusqlite::Connection,
    node_id: &str,
    resolution: Resolution,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<PointSeries, TcpConnectionHistoryError> {
    let mut stmt = conn.prepare(
        "SELECT bucket_unix, class, subject, min, avg, max, samples_expected, samples_actual,
                truncated, partial
         FROM rollup_bucket
         WHERE node_id = ?1 AND resolution = ?2 AND bucket_unix >= ?3 AND bucket_unix <= ?4
         ORDER BY bucket_unix",
    )?;
    let rows = stmt.query_map(
        rusqlite::params![
            node_id,
            resolution.as_str(),
            start.timestamp(),
            end.timestamp()
        ],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, f64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, i64>(9)?,
            ))
        },
    )?;

    let mut series = PointSeries::default();
    for row in rows {
        let (bucket_unix, class_raw, subject, min, avg, max, expected, actual, truncated, partial) =
            row?;
        let Some(bucket_start) = DateTime::from_timestamp(bucket_unix, 0) else {
            continue;
        };
        let point = Point {
            bucket_start,
            min: min.max(0) as u32,
            avg,
            max: max.max(0) as u32,
            samples_expected: expected.max(0) as u32,
            samples_actual: actual.max(0) as u32,
            truncated: truncated != 0,
            partial: partial != 0,
        };
        if class_raw == "*" && subject.is_empty() {
            series.total.push(point);
            continue;
        }
        let Some(class) = ConnectionClass::parse(&class_raw) else {
            continue;
        };
        if subject.is_empty() {
            series.classes.entry(class).or_default().push(point);
        } else {
            series
                .subjects
                .entry((class, subject))
                .or_default()
                .push(point);
        }
    }
    Ok(series)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::tcp_connection_history::test_support::stored_minute_split as minute;

    #[test]
    fn total_extrema_are_computed_from_simultaneous_sums() {
        let minutes = vec![
            minute("2026-11-26T10:00:00Z", 5, 5, 0),
            minute("2026-11-26T10:01:00Z", 3, 0, 3),
        ];
        let series = aggregate_points(
            &minutes,
            Resolution::FiveMinute,
            "2026-11-26T10:00:00Z".parse().unwrap(),
            "2026-11-26T10:04:00Z".parse().unwrap(),
        );
        assert_eq!(series.total.len(), 1);
        let total = &series.total[0];
        assert_eq!(total.max, 5);
        assert_eq!(total.min, 3);
        assert_eq!(total.samples_expected, 5);
        assert_eq!(total.samples_actual, 2);
        assert_eq!(series.classes[&ConnectionClass::Inbound][0].max, 5);
        assert_eq!(series.classes[&ConnectionClass::OutboundOther][0].max, 3);
    }

    #[test]
    fn daily_bucket_uses_nominal_expected_minutes() {
        let minutes = vec![minute("2026-11-26T00:00:00Z", 1, 1, 0)];
        let series = aggregate_points(
            &minutes,
            Resolution::Daily,
            "2026-11-26T00:00:00Z".parse().unwrap(),
            "2026-11-26T00:00:00Z".parse().unwrap(),
        );
        assert_eq!(series.total[0].samples_expected, 1);
        assert_eq!(series.total[0].samples_actual, 1);
    }

    #[test]
    fn gaps_lower_actual_without_lowering_expected() {
        let minutes = vec![minute("2026-11-26T10:00:00Z", 2, 2, 0)];
        let series = aggregate_points(
            &minutes,
            Resolution::FiveMinute,
            "2026-11-26T10:00:00Z".parse().unwrap(),
            "2026-11-26T10:04:00Z".parse().unwrap(),
        );
        assert_eq!(series.total[0].samples_expected, 5);
        assert_eq!(series.total[0].samples_actual, 1);
    }
}
