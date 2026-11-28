use std::{collections::BTreeMap, io, path::Path};

use chrono::{DateTime, Duration, Timelike as _, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{
    config::Config,
    domain::Endpoint,
    tcp_connection_usage::{
        PersistedTcpConnectionUsage, TcpConnectionMinuteSample, TcpConnectionUsageWarning,
    },
};

mod collector;
mod rollup;
#[cfg(test)]
mod test_support;
mod wire;

pub use collector::{
    ControlListener, ProcTcpRow, collect_full_site_minute, control_listeners, resolve_runtime_uids,
};
pub use rollup::{
    Point, PointSeries, Resolution, aggregate_points, floor_bucket, load_rollups, minute_series,
};
pub use wire::{
    DailyBucketPayload, REPOSITORY_CAPABILITY, REPOSITORY_RETENTION_DAYS,
    TCP_CONNECTION_HISTORY_SCHEMA, TCP_CONNECTION_HISTORY_STREAM, daily_payload_from_series,
    daily_record_key, parse_daily_payload, series_from_daily_payloads,
};

pub const MINUTES_WINDOW: usize = 7 * 24 * 60;
pub const MAX_ESTABLISHED_TCP_CONNECTIONS: usize = 16_384;
const STORE_FILE: &str = "tcp_connections.sqlite3";
const LEGACY_IMPORT_MARKER: &str = "legacy_import_v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ConnectionClass {
    #[serde(rename = "inbound")]
    Inbound,
    #[serde(rename = "control_plane")]
    ControlPlane,
    #[serde(rename = "outbound_runtime")]
    OutboundRuntime,
    #[serde(rename = "outbound_other")]
    OutboundOther,
}

impl ConnectionClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbound => "inbound",
            Self::ControlPlane => "control_plane",
            Self::OutboundRuntime => "outbound_runtime",
            Self::OutboundOther => "outbound_other",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "inbound" => Some(Self::Inbound),
            "control_plane" => Some(Self::ControlPlane),
            "outbound_runtime" => Some(Self::OutboundRuntime),
            "outbound_other" => Some(Self::OutboundOther),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullSiteMinuteSample {
    pub counts: BTreeMap<(ConnectionClass, String), u32>,
    pub total: u32,
    pub truncated: bool,
    pub partial: bool,
    pub classes_observed: Vec<ConnectionClass>,
}

impl FullSiteMinuteSample {
    pub fn new(
        counts: BTreeMap<(ConnectionClass, String), u32>,
        truncated: bool,
        partial: bool,
        classes_observed: Vec<ConnectionClass>,
    ) -> Self {
        let total = counts.values().copied().sum();
        Self {
            counts,
            total,
            truncated,
            partial,
            classes_observed,
        }
    }

    pub fn class_total(&self, class: ConnectionClass) -> u32 {
        self.counts
            .iter()
            .filter(|((item_class, _), _)| *item_class == class)
            .map(|(_, count)| *count)
            .sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMinute {
    pub minute: DateTime<Utc>,
    pub total: u32,
    pub truncated: bool,
    pub partial: bool,
    pub classes_observed: Vec<ConnectionClass>,
    pub counts: BTreeMap<(ConnectionClass, String), u32>,
}

#[derive(Debug)]
pub enum TcpConnectionHistoryError {
    Io(io::Error),
    Sqlite(rusqlite::Error),
    Unsupported(String),
}

impl std::fmt::Display for TcpConnectionHistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Sqlite(err) => write!(f, "sqlite error: {err}"),
            Self::Unsupported(message) => write!(f, "unsupported: {message}"),
        }
    }
}

impl std::error::Error for TcpConnectionHistoryError {}

impl From<io::Error> for TcpConnectionHistoryError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<rusqlite::Error> for TcpConnectionHistoryError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

pub struct TcpConnectionHistoryStore {
    conn: Connection,
}

impl TcpConnectionHistoryStore {
    pub fn open(data_dir: &Path) -> Result<Self, TcpConnectionHistoryError> {
        let conn = Connection::open(data_dir.join(STORE_FILE))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS minute_header (
                 node_id TEXT NOT NULL,
                 minute_unix INTEGER NOT NULL,
                 total INTEGER NOT NULL,
                 truncated INTEGER NOT NULL,
                 partial INTEGER NOT NULL,
                 classes_observed TEXT NOT NULL,
                 PRIMARY KEY (node_id, minute_unix)
             );
             CREATE TABLE IF NOT EXISTS minute_subject (
                 node_id TEXT NOT NULL,
                 minute_unix INTEGER NOT NULL,
                 class TEXT NOT NULL,
                 subject TEXT NOT NULL,
                 count INTEGER NOT NULL,
                 PRIMARY KEY (node_id, minute_unix, class, subject)
             );
             CREATE INDEX IF NOT EXISTS minute_subject_lookup
                 ON minute_subject (node_id, minute_unix);
             CREATE TABLE IF NOT EXISTS rollup_bucket (
                 node_id TEXT NOT NULL,
                 resolution TEXT NOT NULL,
                 bucket_unix INTEGER NOT NULL,
                 class TEXT NOT NULL,
                 subject TEXT NOT NULL,
                 min INTEGER NOT NULL,
                 avg REAL NOT NULL,
                 max INTEGER NOT NULL,
                 samples_expected INTEGER NOT NULL,
                 samples_actual INTEGER NOT NULL,
                 truncated INTEGER NOT NULL,
                 partial INTEGER NOT NULL,
                 delivered INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (node_id, resolution, bucket_unix, class, subject)
             );
             CREATE INDEX IF NOT EXISTS rollup_lookup
                 ON rollup_bucket (node_id, resolution, bucket_unix);
             CREATE TABLE IF NOT EXISTS history_metadata (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );",
        )?;
        Ok(Self { conn })
    }

    pub fn record_minute(
        &mut self,
        node_id: &str,
        minute: DateTime<Utc>,
        sample: &FullSiteMinuteSample,
    ) -> Result<(), TcpConnectionHistoryError> {
        let minute = floor_minute(minute);
        let minute_unix = minute.timestamp();
        let observed = serde_json::to_string(
            &sample
                .classes_observed
                .iter()
                .map(|class| class.as_str())
                .collect::<Vec<_>>(),
        )
        .map_err(|err| TcpConnectionHistoryError::Unsupported(err.to_string()))?;

        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM minute_header WHERE node_id = ?1 AND minute_unix = ?2",
            params![node_id, minute_unix],
        )?;
        tx.execute(
            "DELETE FROM minute_subject WHERE node_id = ?1 AND minute_unix = ?2",
            params![node_id, minute_unix],
        )?;
        tx.execute(
            "INSERT INTO minute_header
             (node_id, minute_unix, total, truncated, partial, classes_observed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                node_id,
                minute_unix,
                i64::from(sample.total),
                sample.truncated as i64,
                sample.partial as i64,
                observed
            ],
        )?;
        for ((class, subject), count) in &sample.counts {
            if *count == 0 {
                continue;
            }
            tx.execute(
                "INSERT INTO minute_subject (node_id, minute_unix, class, subject, count)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    node_id,
                    minute_unix,
                    class.as_str(),
                    subject.as_str(),
                    i64::from(*count)
                ],
            )?;
        }
        let cutoff = (minute - Duration::days(7)).timestamp();
        tx.execute(
            "DELETE FROM minute_header WHERE node_id = ?1 AND minute_unix < ?2",
            params![node_id, cutoff],
        )?;
        tx.execute(
            "DELETE FROM minute_subject WHERE node_id = ?1 AND minute_unix < ?2",
            params![node_id, cutoff],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Imports the legacy inbound-only JSON view once. Existing full-class minutes are never
    /// overwritten, and the mirror stops being a data source after the marker is written.
    pub fn import_legacy(
        &mut self,
        node_id: &str,
        usage: &PersistedTcpConnectionUsage,
    ) -> Result<bool, TcpConnectionHistoryError> {
        let tx = self.conn.transaction()?;
        let marker: Option<String> = tx
            .query_row(
                "SELECT value FROM history_metadata WHERE key = ?1",
                params![LEGACY_IMPORT_MARKER],
                |row| row.get(0),
            )
            .optional()?;
        if marker.is_some() {
            return Ok(false);
        }
        let Some(latest) = usage.latest_minute_dt() else {
            tx.execute(
                "INSERT OR REPLACE INTO history_metadata (key, value) VALUES (?1, ?2)",
                params![LEGACY_IMPORT_MARKER, "1"],
            )?;
            tx.commit()?;
            return Ok(false);
        };

        let mut inserted = false;
        for index in 0..MINUTES_WINDOW {
            let minute = latest - Duration::minutes((MINUTES_WINDOW - 1 - index) as i64);
            let minute_unix = minute.timestamp();
            let exists: Option<i64> = tx
                .query_row(
                    "SELECT 1 FROM minute_header WHERE node_id = ?1 AND minute_unix = ?2",
                    params![node_id, minute_unix],
                    |row| row.get(0),
                )
                .optional()?;
            if exists.is_some() {
                continue;
            }

            let mut counts = BTreeMap::new();
            let mut total = 0_u32;
            for endpoint in usage
                .endpoints
                .values()
                .filter(|endpoint| endpoint.node_id == node_id)
            {
                let Some(count) = endpoint
                    .counts
                    .get(index)
                    .copied()
                    .filter(|count| *count > 0)
                else {
                    continue;
                };
                total = total.saturating_add(u32::from(count));
                counts.insert(
                    (ConnectionClass::Inbound, endpoint.endpoint_id.clone()),
                    u32::from(count),
                );
            }
            if total == 0 {
                continue;
            }
            tx.execute(
                "INSERT INTO minute_header
                 (node_id, minute_unix, total, truncated, partial, classes_observed)
                 VALUES (?1, ?2, ?3, 0, 1, ?4)",
                params![
                    node_id,
                    minute_unix,
                    i64::from(total),
                    serde_json::json!(["inbound"]).to_string()
                ],
            )?;
            for ((class, subject), count) in counts {
                tx.execute(
                    "INSERT INTO minute_subject (node_id, minute_unix, class, subject, count)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        node_id,
                        minute_unix,
                        class.as_str(),
                        subject.as_str(),
                        i64::from(count)
                    ],
                )?;
            }
            inserted = true;
        }
        tx.execute(
            "INSERT OR REPLACE INTO history_metadata (key, value) VALUES (?1, ?2)",
            params![LEGACY_IMPORT_MARKER, "1"],
        )?;
        tx.commit()?;
        Ok(inserted)
    }

    pub fn load_minutes(
        &self,
        node_id: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<StoredMinute>, TcpConnectionHistoryError> {
        let headers = {
            let mut stmt = self.conn.prepare(
                "SELECT minute_unix, total, truncated, partial, classes_observed
                 FROM minute_header
                 WHERE node_id = ?1 AND minute_unix >= ?2 AND minute_unix <= ?3
                 ORDER BY minute_unix",
            )?;
            let rows = stmt.query_map(
                params![node_id, start.timestamp(), end.timestamp()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let mut minutes = Vec::with_capacity(headers.len());
        for (minute_unix, total, truncated, partial, observed) in headers {
            let Some(minute) = DateTime::from_timestamp(minute_unix, 0) else {
                continue;
            };
            let classes_observed = serde_json::from_str::<Vec<String>>(&observed)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|class| ConnectionClass::parse(&class))
                .collect::<Vec<_>>();
            let counts = {
                let mut stmt = self.conn.prepare(
                    "SELECT class, subject, count
                     FROM minute_subject
                     WHERE node_id = ?1 AND minute_unix = ?2",
                )?;
                let rows = stmt.query_map(params![node_id, minute_unix], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })?;
                let mut counts = BTreeMap::new();
                for row in rows {
                    let (class, subject, count) = row?;
                    let Some(class) = ConnectionClass::parse(&class) else {
                        continue;
                    };
                    counts.insert((class, subject), count.max(0) as u32);
                }
                counts
            };
            minutes.push(StoredMinute {
                minute,
                total: total.max(0) as u32,
                truncated: truncated != 0,
                partial: partial != 0,
                classes_observed,
                counts,
            });
        }
        Ok(minutes)
    }

    /// Rebuild only the rollup buckets touched by one confirmed minute, then prune expired
    /// rollups. Full-retention rebuilds would rescan 10k raw minutes every minute.
    pub fn refresh_rollups_for_minute(
        &mut self,
        node_id: &str,
        minute: DateTime<Utc>,
    ) -> Result<(), TcpConnectionHistoryError> {
        let minute = floor_minute(minute);
        for (resolution, retention_days) in
            [(Resolution::FiveMinute, 7_i64), (Resolution::Daily, 90_i64)]
        {
            let bucket_start = floor_bucket(minute, resolution);
            let bucket_end = bucket_start + Duration::minutes(resolution.minutes() - 1);
            let raw = self.load_minutes(node_id, bucket_start, bucket_end)?;
            let series = aggregate_points(&raw, resolution, bucket_start, bucket_end);

            let tx = self.conn.transaction()?;
            tx.execute(
                "DELETE FROM rollup_bucket
                 WHERE node_id = ?1 AND resolution = ?2 AND bucket_unix = ?3",
                params![node_id, resolution.as_str(), bucket_start.timestamp()],
            )?;
            {
                let mut insert = tx.prepare(
                    "INSERT INTO rollup_bucket
                     (node_id, resolution, bucket_unix, class, subject, min, avg, max,
                      samples_expected, samples_actual, truncated, partial, delivered)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0)",
                )?;
                let mut write_points =
                    |class: &str, subject: &str, points: &[Point]| -> Result<(), rusqlite::Error> {
                        for point in points {
                            insert.execute(params![
                                node_id,
                                resolution.as_str(),
                                bucket_start.timestamp(),
                                class,
                                subject,
                                i64::from(point.min),
                                point.avg,
                                i64::from(point.max),
                                i64::from(point.samples_expected),
                                i64::from(point.samples_actual),
                                point.truncated as i64,
                                point.partial as i64
                            ])?;
                        }
                        Ok(())
                    };
                write_points("*", "", &series.total)?;
                for (class, points) in &series.classes {
                    write_points(class.as_str(), "", points)?;
                }
                for ((class, subject), points) in &series.subjects {
                    write_points(class.as_str(), subject, points)?;
                }
            }
            tx.execute(
                "DELETE FROM rollup_bucket
                 WHERE node_id = ?1 AND resolution = ?2 AND bucket_unix < ?3",
                params![
                    node_id,
                    resolution.as_str(),
                    floor_bucket(minute - Duration::days(retention_days), resolution).timestamp()
                ],
            )?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn load_rollup_series(
        &self,
        node_id: &str,
        resolution: Resolution,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<PointSeries, TcpConnectionHistoryError> {
        load_rollups(&self.conn, node_id, resolution, start, end)
    }

    pub fn raw_bytes(&self) -> Result<u64, TcpConnectionHistoryError> {
        let pages: i64 = self
            .conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let page_size: i64 = self
            .conn
            .query_row("PRAGMA page_size", [], |row| row.get(0))?;
        Ok(pages.max(0) as u64 * page_size.max(0) as u64)
    }

    pub fn pending_daily_buckets(
        &self,
        node_id: &str,
        limit: usize,
    ) -> Result<Vec<i64>, TcpConnectionHistoryError> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT bucket_unix FROM rollup_bucket
             WHERE node_id = ?1 AND resolution = 'daily' AND delivered = 0
             ORDER BY bucket_unix LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![node_id, limit as i64], |row| row.get::<_, i64>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn mark_daily_buckets_delivered(
        &self,
        node_id: &str,
        buckets: &[i64],
    ) -> Result<(), TcpConnectionHistoryError> {
        let mut stmt = self.conn.prepare(
            "UPDATE rollup_bucket SET delivered = 1
             WHERE node_id = ?1 AND resolution = 'daily' AND bucket_unix = ?2",
        )?;
        for bucket in buckets {
            stmt.execute(params![node_id, bucket])?;
        }
        Ok(())
    }

    pub fn has_minute(
        &self,
        node_id: &str,
        minute: DateTime<Utc>,
    ) -> Result<bool, TcpConnectionHistoryError> {
        let exists: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM minute_header WHERE node_id = ?1 AND minute_unix = ?2",
                params![node_id, floor_minute(minute).timestamp()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(exists.is_some())
    }
}

#[derive(Debug)]
pub struct MinuteSampling {
    pub legacy_samples: Vec<TcpConnectionMinuteSample>,
    pub full_site: Option<FullSiteMinuteSample>,
    pub linux_only: bool,
    pub warning: Option<TcpConnectionUsageWarning>,
}

pub fn sample_connections_minute(
    config: &Config,
    data_dir: &Path,
    node_id: &str,
    endpoints: &[Endpoint],
) -> MinuteSampling {
    if !cfg!(target_os = "linux") {
        return MinuteSampling {
            legacy_samples: Vec::new(),
            full_site: None,
            linux_only: false,
            warning: Some(TcpConnectionUsageWarning {
                code: "unsupported_platform".to_string(),
                message: "TCP connection count history is currently only supported on Linux nodes."
                    .to_string(),
            }),
        };
    }

    let listen_ports = endpoints
        .iter()
        .map(|endpoint| (endpoint.port, endpoint.endpoint_id.clone()))
        .collect::<BTreeMap<_, _>>();
    let (runtime_uids, runtime_unresolved) = resolve_runtime_uids(data_dir);
    match collect_full_site_minute(&listen_ports, &control_listeners(config), &runtime_uids) {
        Ok(mut sample) => {
            if runtime_unresolved {
                sample.partial = true;
            }
            let warning = if sample.truncated {
                Some(TcpConnectionUsageWarning {
                    code: "socket_inspection_truncated".to_string(),
                    message: "TCP socket inspection reached the row bound".to_string(),
                })
            } else if sample.partial {
                Some(TcpConnectionUsageWarning {
                    code: if runtime_unresolved {
                        "runtime_identity_unresolved"
                    } else {
                        "socket_inspection_partial"
                    }
                    .to_string(),
                    message: "TCP socket inspection was partial".to_string(),
                })
            } else {
                None
            };
            let counts_by_endpoint = sample
                .counts
                .iter()
                .filter_map(|((class, subject), count)| {
                    (*class == ConnectionClass::Inbound).then_some((subject.clone(), *count))
                })
                .collect::<BTreeMap<_, _>>();
            let legacy_samples = endpoints
                .iter()
                .map(|endpoint| TcpConnectionMinuteSample {
                    node_id: node_id.to_string(),
                    endpoint_id: endpoint.endpoint_id.clone(),
                    endpoint_tag: endpoint.tag.clone(),
                    port: endpoint.port,
                    count: counts_by_endpoint
                        .get(&endpoint.endpoint_id)
                        .copied()
                        .unwrap_or(0),
                })
                .collect();
            MinuteSampling {
                legacy_samples,
                full_site: Some(sample),
                linux_only: true,
                warning,
            }
        }
        Err(TcpConnectionHistoryError::Unsupported(message)) => MinuteSampling {
            legacy_samples: Vec::new(),
            full_site: None,
            linux_only: false,
            warning: Some(TcpConnectionUsageWarning {
                code: "unsupported_platform".to_string(),
                message,
            }),
        },
        Err(err) => MinuteSampling {
            legacy_samples: Vec::new(),
            full_site: None,
            linux_only: true,
            warning: Some(TcpConnectionUsageWarning {
                code: "socket_inspection_failed".to_string(),
                message: format!("Failed to inspect local TCP socket state: {err}"),
            }),
        },
    }
}

/// Reads at most `limit` undelivered daily buckets and renders their repository payloads.
pub fn pending_daily_payloads(
    data_dir: &Path,
    node_id: &str,
    limit: usize,
) -> Result<Vec<(i64, serde_json::Value)>, TcpConnectionHistoryError> {
    let store = TcpConnectionHistoryStore::open(data_dir)?;
    let buckets = store.pending_daily_buckets(node_id, limit)?;
    let mut payloads = Vec::with_capacity(buckets.len());
    for bucket in buckets {
        let Some(start) = DateTime::from_timestamp(bucket, 0) else {
            continue;
        };
        let end = start + Duration::days(1) - Duration::minutes(1);
        let series = store.load_rollup_series(node_id, Resolution::Daily, start, end)?;
        let Some(payload) = daily_payload_from_series(&series, start) else {
            continue;
        };
        payloads.push((
            bucket,
            serde_json::to_value(payload)
                .map_err(|err| TcpConnectionHistoryError::Unsupported(err.to_string()))?,
        ));
    }
    Ok(payloads)
}

pub fn mark_daily_payloads_delivered(
    data_dir: &Path,
    node_id: &str,
    buckets: &[i64],
) -> Result<(), TcpConnectionHistoryError> {
    if buckets.is_empty() {
        return Ok(());
    }
    TcpConnectionHistoryStore::open(data_dir)?.mark_daily_buckets_delivered(node_id, buckets)
}

/// Query-time subject filter built from the Raft-backed deleted-endpoints registry.
pub fn deleted_subjects(
    deleted_endpoints: &std::collections::BTreeMap<String, crate::state::DeletedEndpointRecord>,
    node_id: &str,
) -> BTreeMap<ConnectionClass, Vec<String>> {
    let mut deleted = BTreeMap::<ConnectionClass, Vec<String>>::new();
    for (endpoint_id, record) in deleted_endpoints {
        if record.node_id == node_id {
            deleted
                .entry(ConnectionClass::Inbound)
                .or_default()
                .push(endpoint_id.clone());
        }
    }
    deleted
}

/// Removes registry-covered subjects from a series while leaving total and classes untouched.
pub fn filter_deleted_subjects(
    series: &mut PointSeries,
    deleted: &BTreeMap<ConnectionClass, Vec<String>>,
) {
    series.subjects.retain(|(class, subject), _| {
        !deleted
            .get(class)
            .is_some_and(|subjects| subjects.iter().any(|item| item == subject))
    });
}

/// Local series for one node and window. Returns the raw-minute series for 24h, the stored
/// rollup for 5m, and the stored rollup for daily.
pub fn local_series(
    data_dir: &Path,
    node_id: &str,
    window: Duration,
    resolution: Option<Resolution>,
) -> Result<PointSeries, TcpConnectionHistoryError> {
    let store = TcpConnectionHistoryStore::open(data_dir)?;
    let now = Utc::now();
    let start = floor_minute(now) - window + Duration::minutes(1);
    match resolution {
        None => Ok(minute_series(&store.load_minutes(node_id, start, now)?)),
        Some(resolution) => {
            let bucket_start = floor_bucket(start, resolution);
            store.load_rollup_series(node_id, resolution, bucket_start, floor_minute(now))
        }
    }
}

pub fn persist_connections_minute(
    data_dir: &Path,
    node_id: &str,
    minute: DateTime<Utc>,
    legacy: &PersistedTcpConnectionUsage,
    sample: Option<&FullSiteMinuteSample>,
) -> Result<bool, TcpConnectionHistoryError> {
    let mut store = TcpConnectionHistoryStore::open(data_dir)?;
    store.import_legacy(node_id, legacy)?;
    let Some(sample) = sample else {
        return Ok(true);
    };
    // Budget guard: stop writing new minutes near the 96 MiB local ceiling.
    if store.raw_bytes()?.saturating_mul(100) >= 96 * 1024 * 1024 * 80 {
        return Ok(false);
    }
    store.record_minute(node_id, minute, sample)?;
    store.refresh_rollups_for_minute(node_id, minute)?;
    Ok(true)
}

pub fn floor_minute(at: DateTime<Utc>) -> DateTime<Utc> {
    at.with_second(0)
        .and_then(|value| value.with_nanosecond(0))
        .unwrap_or(at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tcp_connection_history::test_support::deleted_registry;
    use crate::tcp_connection_history::test_support::legacy_usage;

    #[test]
    fn imports_legacy_minutes_once_and_never_overwrites_full_class_data() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = TcpConnectionHistoryStore::open(dir.path()).unwrap();
        let minute = floor_minute(Utc::now());
        let usage = legacy_usage(minute);
        let node = xp_test_fixtures::label_node1_variant2();
        let endpoint = xp_test_fixtures::endpoint_fixtureinline_a();

        assert!(store.import_legacy(node, &usage).unwrap());
        let minutes = store.load_minutes(node, minute, minute).unwrap();
        assert_eq!(minutes.len(), 1);
        assert_eq!(minutes[0].total, 3);
        assert!(minutes[0].partial);
        assert_eq!(
            minutes[0].counts[&(ConnectionClass::Inbound, endpoint.to_string())],
            3
        );

        let full = FullSiteMinuteSample::new(
            BTreeMap::from([((ConnectionClass::Inbound, endpoint.to_string()), 5_u32)]),
            false,
            false,
            vec![ConnectionClass::Inbound],
        );
        store.record_minute(node, minute, &full).unwrap();
        assert!(!store.import_legacy(node, &usage).unwrap());
        let minutes = store.load_minutes(node, minute, minute).unwrap();
        assert_eq!(minutes[0].total, 5);
        assert!(!minutes[0].partial);
    }

    #[test]
    fn rollups_persist_total_and_class_points_without_summing_subject_maxima() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = TcpConnectionHistoryStore::open(dir.path()).unwrap();
        let minute = floor_bucket(floor_minute(Utc::now()), Resolution::FiveMinute);
        let node = xp_test_fixtures::label_node1_variant2();
        let endpoint = xp_test_fixtures::endpoint_fixtureinline_a();
        let first = FullSiteMinuteSample::new(
            BTreeMap::from([
                ((ConnectionClass::Inbound, endpoint.to_string()), 4_u32),
                ((ConnectionClass::OutboundOther, "0".to_string()), 1_u32),
            ]),
            false,
            false,
            vec![ConnectionClass::Inbound, ConnectionClass::OutboundOther],
        );
        store.record_minute(node, minute, &first).unwrap();
        let second = FullSiteMinuteSample::new(
            BTreeMap::from([((ConnectionClass::Inbound, endpoint.to_string()), 0_u32)]),
            false,
            false,
            vec![ConnectionClass::Inbound],
        );
        store
            .record_minute(node, minute + Duration::minutes(1), &second)
            .unwrap();
        store
            .refresh_rollups_for_minute(node, minute + Duration::minutes(1))
            .unwrap();
        store.refresh_rollups_for_minute(node, minute).unwrap();

        let start = floor_bucket(minute, Resolution::FiveMinute);
        let series = store
            .load_rollup_series(node, Resolution::FiveMinute, start, start)
            .unwrap();
        assert_eq!(series.total.len(), 1);
        assert_eq!(series.total[0].max, 5);
        assert_eq!(series.total[0].samples_actual, 2);
        assert_eq!(series.classes[&ConnectionClass::Inbound][0].max, 4);
        let daily = store
            .load_rollup_series(
                node,
                Resolution::Daily,
                floor_bucket(minute, Resolution::Daily),
                floor_bucket(minute, Resolution::Daily),
            )
            .unwrap();
        assert_eq!(daily.total.len(), 1);
        assert!(daily.total[0].samples_expected >= 1);
    }

    #[test]
    fn budget_guard_keeps_retained_minutes_after_the_local_ceiling() {
        assert_eq!(16_384_usize * 6, 98_304);
        let ceiling = 96_u64 * 1024 * 1024;
        assert!(ceiling / 100 * 80 > 76 * 1024 * 1024);
    }

    #[test]
    fn deleted_endpoint_subjects_are_removed_without_touching_total() {
        let mut series = PointSeries::default();
        let start = DateTime::from_timestamp(1_790_000_000, 0).unwrap();
        series.total.push(Point {
            bucket_start: start,
            min: 2,
            avg: 2.0,
            max: 2,
            samples_expected: 1,
            samples_actual: 1,
            truncated: false,
            partial: false,
        });
        series.subjects.insert(
            (
                ConnectionClass::Inbound,
                xp_test_fixtures::endpoint_fixtureinline_a().to_string(),
            ),
            Vec::new(),
        );
        series.subjects.insert(
            (
                ConnectionClass::Inbound,
                xp_test_fixtures::endpoint_fixtureinline_b().to_string(),
            ),
            Vec::new(),
        );
        let registry = deleted_registry();
        let deleted = deleted_subjects(&registry, xp_test_fixtures::label_node1_variant2());
        filter_deleted_subjects(&mut series, &deleted);
        assert!(!series.subjects.contains_key(&(
            ConnectionClass::Inbound,
            xp_test_fixtures::endpoint_fixtureinline_a().to_string()
        )));
        assert!(series.subjects.contains_key(&(
            ConnectionClass::Inbound,
            xp_test_fixtures::endpoint_fixtureinline_b().to_string()
        )));
        assert_eq!(series.total[0].max, 2);
    }

    #[test]
    fn registry_filter_is_scoped_to_the_node() {
        let registry = deleted_registry();
        let deleted = deleted_subjects(&registry, xp_test_fixtures::label_node1_variant2());
        assert_eq!(
            deleted[&ConnectionClass::Inbound],
            vec![xp_test_fixtures::endpoint_fixtureinline_a().to_string()]
        );
    }
}
