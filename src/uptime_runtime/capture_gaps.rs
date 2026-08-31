use rusqlite::{OptionalExtension, Row, params};

use crate::{
    id::new_ulid_string,
    uptime_monitor::{ExpectedSlotRange, ServiceMonitor, normalized_observer_set},
};

use super::{MAX_PENDING_CAPTURE_GAPS, UptimeHandle, UptimeRuntime};

#[derive(Debug, Clone)]
pub struct PendingCaptureGap {
    pub id: String,
    pub monitor_id: String,
    pub revision: u64,
    pub observer_node_id: String,
    pub range: ExpectedSlotRange,
}

impl UptimeHandle {
    /// Records that a scheduled local observer could not execute a slot. The row coalesces
    /// contiguous slots before delivery, so a prolonged capture suspension remains bounded.
    pub async fn record_capture_gap(
        &self,
        monitor: &ServiceMonitor,
        observer_node_id: String,
        observer_set_node_ids: Vec<String>,
        slot_unix_seconds: u64,
    ) -> Result<bool, rusqlite::Error> {
        let observer_set_node_ids = normalized_observer_set(&observer_set_node_ids)
            .unwrap_or_else(|| vec![observer_node_id.clone()]);
        let observer_set_node_ids_json = serde_json::to_vec(&observer_set_node_ids)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let mut runtime = self.inner.lock().await;
        runtime.refresh_capture_state()?;
        let interval_seconds = i64::from(monitor.interval_seconds);
        let previous_end = i64::try_from(slot_unix_seconds)
            .unwrap_or(i64::MAX)
            .saturating_sub(interval_seconds);
        let existing_id: Option<String> = runtime
            .connection
            .query_row(
                "SELECT id FROM uptime_capture_gaps
                 WHERE monitor_id = ?1 AND revision = ?2 AND observer_node_id = ?3
                   AND interval_seconds = ?4 AND observer_set_node_ids_json = ?5
                   AND enqueued = 0 AND end_slot_unix_seconds = ?6
                 ORDER BY id DESC LIMIT 1",
                params![
                    monitor.monitor_id,
                    i64::try_from(monitor.revision).unwrap_or(i64::MAX),
                    observer_node_id,
                    interval_seconds,
                    observer_set_node_ids_json,
                    previous_end,
                ],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(id) = existing_id {
            return runtime
                .connection
                .execute(
                    "UPDATE uptime_capture_gaps SET end_slot_unix_seconds = ?2 WHERE id = ?1",
                    params![id, i64::try_from(slot_unix_seconds).unwrap_or(i64::MAX)],
                )
                .map(|changed| changed == 1);
        }
        if runtime.capture_gaps_suspended
            || runtime.pending_capture_gap_count()? >= MAX_PENDING_CAPTURE_GAPS
        {
            runtime.capture_gaps_suspended = true;
            return Ok(false);
        }
        runtime.connection.execute(
            "INSERT INTO uptime_capture_gaps
             (id, monitor_id, revision, observer_node_id, interval_seconds,
              observer_set_node_ids_json, start_slot_unix_seconds, end_slot_unix_seconds)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                new_ulid_string(),
                monitor.monitor_id,
                i64::try_from(monitor.revision).unwrap_or(i64::MAX),
                observer_node_id,
                interval_seconds,
                observer_set_node_ids_json,
                i64::try_from(slot_unix_seconds).unwrap_or(i64::MAX),
                i64::try_from(slot_unix_seconds).unwrap_or(i64::MAX),
            ],
        )?;
        runtime.refresh_capture_state()?;
        Ok(true)
    }

    pub async fn pending_capture_gaps(
        &self,
        limit: usize,
    ) -> Result<Vec<PendingCaptureGap>, rusqlite::Error> {
        let runtime = self.inner.lock().await;
        let mut statement = runtime.connection.prepare(
            "SELECT id, monitor_id, revision, observer_node_id, interval_seconds,
                    observer_set_node_ids_json, start_slot_unix_seconds, end_slot_unix_seconds
             FROM uptime_capture_gaps WHERE enqueued = 0
             ORDER BY end_slot_unix_seconds, id LIMIT ?1",
        )?;
        statement
            .query_map(
                [i64::try_from(limit).unwrap_or(i64::MAX)],
                capture_gap_from_row,
            )?
            .collect()
    }

    pub async fn capture_gaps(
        &self,
        monitor_id: &str,
        start_unix_seconds: u64,
        end_unix_seconds: u64,
        limit: usize,
    ) -> Result<Vec<PendingCaptureGap>, rusqlite::Error> {
        let runtime = self.inner.lock().await;
        let mut statement = runtime.connection.prepare(
            "SELECT id, monitor_id, revision, observer_node_id, interval_seconds,
                    observer_set_node_ids_json, start_slot_unix_seconds, end_slot_unix_seconds
             FROM uptime_capture_gaps
             WHERE monitor_id = ?1
               AND end_slot_unix_seconds >= ?2
               AND start_slot_unix_seconds <= ?3
             ORDER BY end_slot_unix_seconds, id LIMIT ?4",
        )?;
        statement
            .query_map(
                params![
                    monitor_id,
                    i64::try_from(start_unix_seconds).unwrap_or(i64::MAX),
                    i64::try_from(end_unix_seconds).unwrap_or(i64::MAX),
                    i64::try_from(limit).unwrap_or(i64::MAX),
                ],
                capture_gap_from_row,
            )?
            .collect()
    }

    pub async fn mark_capture_gaps_enqueued(&self, ids: &[String]) -> Result<(), rusqlite::Error> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut runtime = self.inner.lock().await;
        let transaction = runtime.connection.transaction()?;
        for id in ids {
            transaction.execute(
                "UPDATE uptime_capture_gaps SET enqueued = 1 WHERE id = ?1",
                [id],
            )?;
        }
        transaction.commit()
    }
}

fn capture_gap_from_row(row: &Row<'_>) -> Result<PendingCaptureGap, rusqlite::Error> {
    let observer_set_node_ids_json: Vec<u8> = row.get(5)?;
    let observer_set_node_ids =
        serde_json::from_slice(&observer_set_node_ids_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Blob,
                Box::new(error),
            )
        })?;
    Ok(PendingCaptureGap {
        id: row.get(0)?,
        monitor_id: row.get(1)?,
        revision: u64::try_from(row.get::<_, i64>(2)?).unwrap_or_default(),
        observer_node_id: row.get(3)?,
        range: ExpectedSlotRange {
            interval_seconds: u32::try_from(row.get::<_, i64>(4)?).unwrap_or_default(),
            observer_set_node_ids,
            start_slot_unix_seconds: u64::try_from(row.get::<_, i64>(6)?).unwrap_or_default(),
            end_slot_unix_seconds: u64::try_from(row.get::<_, i64>(7)?).unwrap_or_default(),
        },
    })
}

impl UptimeRuntime {
    pub(super) fn pending_capture_gap_count(&self) -> Result<u64, rusqlite::Error> {
        self.connection.query_row(
            "SELECT COUNT(*) FROM uptime_capture_gaps WHERE enqueued = 0",
            [],
            |row| row.get::<_, u64>(0),
        )
    }
}
