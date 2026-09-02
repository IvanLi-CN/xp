use std::path::Path;
use std::{collections::BTreeMap, fs};

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};

use super::{
    MAX_ALERT_TRANSITIONS, MAX_LOCAL_ROLLUPS, RESOURCE_MINUTE_PAYLOAD_LIMIT, ResourceAlert,
    ResourceGap, ResourceHistoryPayload, ResourcePolicy, ResourceRole, ResourceRollup,
    ResourceSeriesPoint,
};

const MAX_RESOURCE_JOURNAL_ITEMS: usize = 10_000;
const MAX_RESOURCE_JOURNAL_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceCapacityError {
    pub required_bytes: u64,
    pub allowed_bytes: u64,
}

pub fn resource_history_capacity_preflight(
    collectible_nodes: u64,
    history_repository_quota_bytes: u64,
) -> Result<(), ResourceCapacityError> {
    let required_bytes =
        super::RESOURCE_HISTORY_PER_NODE_CAPACITY_BYTES.saturating_mul(collectible_nodes);
    let allowed_bytes = history_repository_quota_bytes
        .saturating_mul(40)
        .saturating_div(100)
        .min(super::RESOURCE_HISTORY_MAX_QUOTA_BYTES);
    if required_bytes > allowed_bytes {
        return Err(ResourceCapacityError {
            required_bytes,
            allowed_bytes,
        });
    }
    Ok(())
}

pub(super) struct ResourceStore {
    connection: Option<Connection>,
}

impl ResourceStore {
    pub(super) fn open(data_dir: &Path) -> rusqlite::Result<Self> {
        fs::create_dir_all(data_dir)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let connection = Connection::open(data_dir.join("resource_metrics.sqlite3"))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS resource_rollups (\n\
                bucket INTEGER PRIMARY KEY,\n\
                payload TEXT NOT NULL\n\
             );\n\
             CREATE TABLE IF NOT EXISTS resource_policy (\n\
                id INTEGER PRIMARY KEY CHECK (id = 1),\n\
                payload TEXT NOT NULL\n\
             );\n\
             CREATE TABLE IF NOT EXISTS resource_alerts (\n\
                id TEXT PRIMARY KEY,\n\
                payload TEXT NOT NULL,\n\
                opened_at INTEGER NOT NULL\n\
             );\n\
             CREATE TABLE IF NOT EXISTS resource_gaps (\n\
                id INTEGER PRIMARY KEY AUTOINCREMENT,\n\
                payload TEXT NOT NULL\n\
             );\n\
             CREATE TABLE IF NOT EXISTS resource_history_pending (\n\
                bucket INTEGER PRIMARY KEY,\n\
                payload TEXT NOT NULL,\n\
                enqueued INTEGER NOT NULL DEFAULT 0\n\
             );\n\
             CREATE INDEX IF NOT EXISTS resource_rollups_bucket\n\
             ON resource_rollups(bucket);",
        )?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS resource_history_gap_pending (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                payload TEXT NOT NULL,
                enqueued INTEGER NOT NULL DEFAULT 0
            );",
        )?;
        Ok(Self {
            connection: Some(connection),
        })
    }

    pub(super) fn memory() -> Self {
        Self { connection: None }
    }

    pub(super) fn is_persistent(&self) -> bool {
        self.connection.is_some()
    }

    pub(super) fn save_rollup(&mut self, rollup: &ResourceRollup) -> rusqlite::Result<()> {
        let Some(connection) = self.connection.as_mut() else {
            return Ok(());
        };
        let payload = serde_json::to_string(rollup)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let tx = connection.transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO resource_rollups (bucket, payload) VALUES (?1, ?2)",
            params![rollup.bucket_start_unix_seconds, payload],
        )?;
        tx.execute(
            "DELETE FROM resource_rollups\n\
             WHERE bucket NOT IN (\n\
                SELECT bucket FROM resource_rollups\n\
                ORDER BY bucket DESC LIMIT ?1\n\
             )",
            params![MAX_LOCAL_ROLLUPS as i64],
        )?;
        let history_payload = serde_json::to_string(&ResourceHistoryPayload::Rollup {
            resolution: "1m".to_string(),
            rollup: rollup.clone(),
        })
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        if history_payload.len() > RESOURCE_MINUTE_PAYLOAD_LIMIT {
            return Err(rusqlite::Error::InvalidParameterName(
                "resource_payload_budget_exceeded".to_string(),
            ));
        }
        tx.execute(
            "INSERT OR REPLACE INTO resource_history_pending (bucket, payload, enqueued)\n\
             VALUES (?1, ?2, 0)",
            params![rollup.bucket_start_unix_seconds, history_payload],
        )?;
        let pending = tx.query_row(
            "SELECT COUNT(*), COALESCE(SUM(length(payload)), 0)\n\
             FROM resource_history_pending WHERE enqueued = 0",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )?;
        let pending_gaps = tx.query_row(
            "SELECT COUNT(*), COALESCE(SUM(length(payload)), 0)\n\
             FROM resource_history_gap_pending WHERE enqueued = 0",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )?;
        if pending.0.saturating_add(pending_gaps.0) > MAX_RESOURCE_JOURNAL_ITEMS as i64
            || pending.1.saturating_add(pending_gaps.1) > MAX_RESOURCE_JOURNAL_BYTES as i64
        {
            return Err(rusqlite::Error::InvalidParameterName(
                "resource_history_journal_full".to_string(),
            ));
        }
        tx.commit()?;
        Ok(())
    }

    pub(super) fn pending_rollups(&self, limit: usize) -> rusqlite::Result<Vec<ResourceRollup>> {
        let Some(connection) = self.connection.as_ref() else {
            return Ok(Vec::new());
        };
        let mut statement = connection.prepare(
            "SELECT payload FROM resource_history_pending\n\
             WHERE enqueued = 0 ORDER BY bucket ASC LIMIT ?1",
        )?;
        let rows =
            statement.query_map(params![limit.min(64) as i64], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let payload = row?;
            match serde_json::from_str(&payload).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })? {
                ResourceHistoryPayload::Rollup { rollup, .. } => Ok(rollup),
                ResourceHistoryPayload::CaptureGap { .. } => {
                    Err(rusqlite::Error::InvalidParameterName(
                        "resource gap is not a rollup".to_string(),
                    ))
                }
            }
        })
        .collect()
    }

    pub(super) fn mark_rollups_enqueued(&mut self, buckets: &[i64]) -> rusqlite::Result<()> {
        let Some(connection) = self.connection.as_mut() else {
            return Ok(());
        };
        let tx = connection.transaction()?;
        for bucket in buckets {
            tx.execute(
                "DELETE FROM resource_history_pending WHERE bucket = ?1",
                params![bucket],
            )?;
        }
        tx.commit()
    }

    pub(super) fn pending_gaps(&self, limit: usize) -> rusqlite::Result<Vec<(i64, ResourceGap)>> {
        let Some(connection) = self.connection.as_ref() else {
            return Ok(Vec::new());
        };
        let mut statement = connection.prepare(
            "SELECT id, payload FROM resource_history_gap_pending
             WHERE enqueued = 0 ORDER BY id ASC LIMIT ?1",
        )?;
        let rows = statement.query_map(params![limit.min(64) as i64], |row| {
            let id = row.get::<_, i64>(0)?;
            let payload = row.get::<_, String>(1)?;
            let gap = serde_json::from_str(&payload).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            Ok((id, gap))
        })?;
        rows.collect()
    }

    pub(super) fn mark_gaps_enqueued(&mut self, ids: &[i64]) -> rusqlite::Result<()> {
        let Some(connection) = self.connection.as_mut() else {
            return Ok(());
        };
        let tx = connection.transaction()?;
        for id in ids {
            tx.execute(
                "DELETE FROM resource_history_gap_pending WHERE id = ?1",
                params![id],
            )?;
        }
        tx.commit()
    }

    pub(super) fn history(
        &self,
        metric: &str,
        role: Option<ResourceRole>,
        limit: usize,
        from: Option<i64>,
        to: Option<i64>,
        resolution: &str,
    ) -> rusqlite::Result<Vec<ResourceSeriesPoint>> {
        let Some(connection) = &self.connection else {
            return Ok(Vec::new());
        };
        let bucket_seconds = match resolution {
            "15m" => 15 * 60,
            "1h" => 60 * 60,
            _ => 60,
        };
        let query_limit = limit
            .saturating_mul((bucket_seconds / 60) as usize)
            .clamp(1, 1_500);
        let mut statement = connection.prepare(
            "SELECT payload FROM resource_rollups\n\
             WHERE (?1 IS NULL OR bucket >= ?1)\n\
               AND (?2 IS NULL OR bucket <= ?2)\n\
             ORDER BY bucket ASC LIMIT ?3",
        )?;
        let rows = statement.query_map(params![from, to, query_limit as i64], |row| {
            row.get::<_, String>(0)
        })?;
        let key = role
            .map(|role| format!("{}.{}", role.as_str(), metric))
            .unwrap_or_else(|| format!("domain.{metric}"));
        let mut aggregates = BTreeMap::<i64, HistoryAggregate>::new();
        for row in rows {
            let rollup: ResourceRollup = serde_json::from_str(&row?).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            let Some(value) = rollup.values.get(&key) else {
                continue;
            };
            let Some(last) = value.last else {
                continue;
            };
            let bucket =
                rollup.bucket_start_unix_seconds.div_euclid(bucket_seconds) * bucket_seconds;
            let aggregate = aggregates.entry(bucket).or_default();
            aggregate.add(value, last, rollup.capability, rollup.captured_samples);
        }
        let mut points = aggregates
            .into_iter()
            .filter_map(|(bucket, aggregate)| {
                DateTime::<Utc>::from_timestamp(bucket, 0).map(|time| ResourceSeriesPoint {
                    observed_at: time.to_rfc3339(),
                    value: aggregate.mean(),
                    capability: aggregate.capability,
                })
            })
            .collect::<Vec<_>>();
        if points.len() > limit {
            points.drain(..points.len() - limit);
        }
        Ok(points)
    }

    pub(super) fn policy(&self) -> rusqlite::Result<ResourcePolicy> {
        let Some(connection) = &self.connection else {
            return Ok(ResourcePolicy::default());
        };
        let payload: Option<String> = connection
            .query_row(
                "SELECT payload FROM resource_policy WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(payload
            .and_then(|payload| serde_json::from_str(&payload).ok())
            .unwrap_or_default())
    }

    pub(super) fn save_gap(&mut self, gap: &ResourceGap) -> rusqlite::Result<()> {
        let Some(connection) = self.connection.as_mut() else {
            return Ok(());
        };
        let payload = serde_json::to_string(gap)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        if payload.len() > RESOURCE_MINUTE_PAYLOAD_LIMIT {
            return Err(rusqlite::Error::InvalidParameterName(
                "resource_payload_budget_exceeded".to_string(),
            ));
        }
        let tx = connection.transaction()?;
        tx.execute(
            "INSERT INTO resource_gaps (payload) VALUES (?1)",
            params![&payload],
        )?;
        tx.execute(
            "INSERT INTO resource_history_gap_pending (payload, enqueued) VALUES (?1, 0)",
            params![&payload],
        )?;
        let pending = tx.query_row(
            "SELECT COUNT(*), COALESCE(SUM(length(payload)), 0)
             FROM resource_history_pending WHERE enqueued = 0",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )?;
        let pending_gaps = tx.query_row(
            "SELECT COUNT(*), COALESCE(SUM(length(payload)), 0)
             FROM resource_history_gap_pending WHERE enqueued = 0",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )?;
        if pending.0.saturating_add(pending_gaps.0) > MAX_RESOURCE_JOURNAL_ITEMS as i64
            || pending.1.saturating_add(pending_gaps.1) > MAX_RESOURCE_JOURNAL_BYTES as i64
        {
            return Err(rusqlite::Error::InvalidParameterName(
                "resource_history_journal_full".to_string(),
            ));
        }
        tx.execute(
            "DELETE FROM resource_gaps WHERE id NOT IN (\n\
             SELECT id FROM resource_gaps ORDER BY id DESC LIMIT ?1\n\
             )",
            params![MAX_ALERT_TRANSITIONS as i64],
        )?;
        tx.commit()
    }

    pub(super) fn gaps(&self) -> rusqlite::Result<Vec<ResourceGap>> {
        let Some(connection) = &self.connection else {
            return Ok(Vec::new());
        };
        let mut statement =
            connection.prepare("SELECT payload FROM resource_gaps ORDER BY id ASC LIMIT ?1")?;
        let rows = statement.query_map(params![MAX_ALERT_TRANSITIONS as i64], |row| {
            row.get::<_, String>(0)
        })?;
        Ok(rows
            .filter_map(|row| {
                row.ok()
                    .and_then(|payload| serde_json::from_str::<ResourceGap>(&payload).ok())
            })
            .collect())
    }

    pub(super) fn save_policy(&mut self, policy: &ResourcePolicy) -> rusqlite::Result<()> {
        let Some(connection) = &self.connection else {
            return Ok(());
        };
        let payload = serde_json::to_string(policy)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        connection.execute(
            "INSERT OR REPLACE INTO resource_policy (id, payload) VALUES (1, ?1)",
            params![payload],
        )?;
        Ok(())
    }

    pub(super) fn alerts(&self) -> rusqlite::Result<Vec<ResourceAlert>> {
        let Some(connection) = &self.connection else {
            return Ok(Vec::new());
        };
        let mut statement = connection
            .prepare("SELECT payload FROM resource_alerts ORDER BY opened_at DESC LIMIT ?1")?;
        let rows = statement.query_map(params![MAX_ALERT_TRANSITIONS as i64], |row| {
            row.get::<_, String>(0)
        })?;
        Ok(rows
            .filter_map(|row| {
                row.ok()
                    .and_then(|payload| serde_json::from_str(&payload).ok())
            })
            .collect())
    }

    pub(super) fn save_alert(&mut self, alert: &ResourceAlert) -> rusqlite::Result<()> {
        let Some(connection) = &self.connection else {
            return Ok(());
        };
        let payload = serde_json::to_string(alert)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        connection.execute(
            "INSERT OR REPLACE INTO resource_alerts (id, payload, opened_at) VALUES (?1, ?2, ?3)",
            params![alert.id, payload, alert.latest_bucket_start_unix_seconds],
        )?;
        connection.execute(
            "DELETE FROM resource_alerts WHERE opened_at < ?1",
            params![
                alert
                    .latest_bucket_start_unix_seconds
                    .saturating_sub(30 * 24 * 60 * 60)
            ],
        )?;
        connection.execute(
            "DELETE FROM resource_alerts WHERE id NOT IN (
                SELECT id FROM resource_alerts ORDER BY opened_at DESC LIMIT ?1
            )",
            params![MAX_ALERT_TRANSITIONS as i64],
        )?;
        Ok(())
    }

    pub(super) fn clear_alert(&mut self, id: &str) -> rusqlite::Result<()> {
        let Some(connection) = &self.connection else {
            return Ok(());
        };
        connection.execute("DELETE FROM resource_alerts WHERE id = ?1", params![id])?;
        Ok(())
    }
}

#[derive(Default)]
struct HistoryAggregate {
    min: Option<f64>,
    max: Option<f64>,
    sum: f64,
    count: u64,
    last: Option<f64>,
    capability: super::Capability,
}

impl HistoryAggregate {
    fn add(
        &mut self,
        value: &super::RollupValue,
        last: f64,
        rollup_capability: super::Capability,
        captured_samples: u32,
    ) {
        if let Some(min) = value.min {
            self.min = Some(self.min.map_or(min, |current| current.min(min)));
        }
        if let Some(max) = value.max {
            self.max = Some(self.max.map_or(max, |current| current.max(max)));
        }
        if let Some(mean) = value.mean {
            let weight = u64::from(captured_samples.max(1));
            self.sum += mean * weight as f64;
            self.count = self.count.saturating_add(weight);
        }
        self.last = Some(last);
        self.capability = self.capability.max(value.capability).max(rollup_capability);
    }

    fn mean(&self) -> Option<f64> {
        (self.count > 0)
            .then(|| self.sum / self.count as f64)
            .or(self.last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource_monitoring::{Capability, RollupValue};

    #[test]
    fn sqlite_migration_is_idempotent() {
        let directory = tempfile::tempdir().unwrap();
        let _ = ResourceStore::open(directory.path()).unwrap();
        let _ = ResourceStore::open(directory.path()).unwrap();
        assert!(directory.path().join("resource_metrics.sqlite3").exists());
    }

    #[test]
    fn rollup_store_has_bounded_pending_rows_and_gap_ack() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = ResourceStore::open(directory.path()).unwrap();
        let rollup = ResourceRollup {
            node_id: xp_test_fixtures::primary_node_id().to_owned(),
            bucket_start_unix_seconds: 60,
            expected_samples: 4,
            captured_samples: 4,
            capability: Capability::Supported,
            values: [(
                "domain.cpu_busy_percent".to_string(),
                RollupValue {
                    min: Some(10.0),
                    mean: Some(20.0),
                    max: Some(30.0),
                    last: Some(25.0),
                    counter_delta: None,
                    capability: Capability::Supported,
                },
            )]
            .into_iter()
            .collect(),
        };
        store.save_rollup(&rollup).unwrap();
        assert_eq!(store.pending_rollups(8).unwrap().len(), 1);
        let gap = ResourceGap {
            from_bucket_start_unix_seconds: 120,
            to_bucket_start_unix_seconds: 180,
            reason_code: "repository_unavailable".to_string(),
        };
        store.save_gap(&gap).unwrap();
        let pending_gaps = store.pending_gaps(8).unwrap();
        assert_eq!(pending_gaps.len(), 1);
        store.mark_gaps_enqueued(&[pending_gaps[0].0]).unwrap();
        assert!(store.pending_gaps(8).unwrap().is_empty());
        let tables: Vec<String> = store
            .connection
            .as_ref()
            .unwrap()
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'resource_%'",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(!tables.iter().any(|name| name.contains("samples")));
    }

    #[test]
    fn history_groups_minute_rollups_at_requested_resolution() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = ResourceStore::open(directory.path()).unwrap();
        for (bucket, value) in [(0_i64, 10.0), (60, 20.0), (900, 40.0)] {
            let rollup = ResourceRollup {
                node_id: xp_test_fixtures::primary_node_id().to_owned(),
                bucket_start_unix_seconds: bucket,
                expected_samples: 4,
                captured_samples: 4,
                capability: Capability::Supported,
                values: [(
                    "domain.cpu_busy_percent".to_string(),
                    RollupValue {
                        min: Some(value - 1.0),
                        mean: Some(value),
                        max: Some(value + 1.0),
                        last: Some(value),
                        counter_delta: None,
                        capability: Capability::Supported,
                    },
                )]
                .into_iter()
                .collect(),
            };
            store.save_rollup(&rollup).unwrap();
        }
        let points = store
            .history("cpu_busy_percent", None, 10, None, None, "15m")
            .unwrap();
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].value, Some(15.0));
        assert_eq!(points[1].value, Some(40.0));
    }
}
