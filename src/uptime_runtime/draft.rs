use rusqlite::{Connection, OptionalExtension, params};

use super::UptimeHandle;

pub const DRAFT_TEST_TTL_SECONDS: u64 = 15 * 60;
const DRAFT_TEST_TOMBSTONE_TTL_SECONDS: u64 = 15 * 60;
const MAX_DRAFT_TEST_TOMBSTONES: i64 = 10_000;

pub(super) fn initialize_idempotency_tables(
    connection: &Connection,
) -> Result<(), rusqlite::Error> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS uptime_draft_test_idempotency (
            caller_fingerprint TEXT NOT NULL,
            key_hash TEXT NOT NULL,
            snapshot_hash TEXT NOT NULL,
            run_id TEXT NOT NULL,
            expires_at_unix_seconds INTEGER NOT NULL,
            PRIMARY KEY (caller_fingerprint, key_hash)
        );
        CREATE INDEX IF NOT EXISTS uptime_draft_test_idempotency_expiry
            ON uptime_draft_test_idempotency (expires_at_unix_seconds);",
    )
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftClusterTestState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Unsupported,
    Interrupted,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DraftClusterTestObserver {
    pub node_id: String,
    pub state: DraftClusterTestState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at_unix_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at_unix_seconds: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct DraftClusterTestObserverUpdate {
    pub node_id: String,
    pub state: DraftClusterTestState,
    pub latency_ms: Option<u32>,
    pub status_code: Option<u16>,
    pub error: Option<String>,
    pub observed_at_unix_seconds: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DraftClusterTest {
    pub run_id: String,
    pub target: crate::uptime_monitor::MonitorTarget,
    pub observer_policy: crate::uptime_monitor::ObserverPolicy,
    pub observer_node_ids: Vec<String>,
    pub coordinator_node_id: String,
    pub state: DraftClusterTestState,
    pub created_at_unix_seconds: u64,
    pub expires_at_unix_seconds: u64,
    pub observers: Vec<DraftClusterTestObserver>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub enum DraftTestCreateOutcome {
    Created(DraftClusterTest),
    Existing(DraftClusterTest),
    IdempotencyConflict,
}

impl UptimeHandle {
    pub async fn create_draft_test(&self, run: &DraftClusterTest) -> Result<(), rusqlite::Error> {
        let runtime = self.inner.lock().await;
        let payload = serde_json::to_vec(run)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        runtime.connection.execute(
            "INSERT INTO uptime_draft_tests (run_id, expires_at_unix_seconds, state, payload)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                run.run_id,
                i64::try_from(run.expires_at_unix_seconds).unwrap_or(i64::MAX),
                draft_test_state_name(&run.state),
                payload,
            ],
        )?;
        Ok(())
    }

    pub async fn create_draft_test_idempotent(
        &self,
        run: &DraftClusterTest,
        caller_fingerprint: &str,
        idempotency_key_hash: Option<&str>,
        snapshot_hash: &str,
        now: u64,
    ) -> Result<DraftTestCreateOutcome, rusqlite::Error> {
        let mut runtime = self.inner.lock().await;
        cleanup_expired_draft_tests(&runtime.connection, now)?;
        let Some(idempotency_key_hash) = idempotency_key_hash else {
            let payload = serde_json::to_vec(run)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
            runtime.connection.execute(
                "INSERT INTO uptime_draft_tests (run_id, expires_at_unix_seconds, state, payload)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    run.run_id,
                    i64::try_from(run.expires_at_unix_seconds).unwrap_or(i64::MAX),
                    draft_test_state_name(&run.state),
                    payload,
                ],
            )?;
            return Ok(DraftTestCreateOutcome::Created(run.clone()));
        };

        let transaction = runtime.connection.transaction()?;
        let existing: Option<(String, String)> = transaction
            .query_row(
                "SELECT run_id, snapshot_hash
                 FROM uptime_draft_test_idempotency
                 WHERE caller_fingerprint = ?1 AND key_hash = ?2
                   AND expires_at_unix_seconds > ?3",
                params![
                    caller_fingerprint,
                    idempotency_key_hash,
                    i64::try_from(now).unwrap_or(i64::MAX),
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((existing_run_id, existing_snapshot_hash)) = existing {
            if existing_snapshot_hash != snapshot_hash {
                transaction.rollback()?;
                return Ok(DraftTestCreateOutcome::IdempotencyConflict);
            }
            let payload: Vec<u8> = transaction.query_row(
                "SELECT payload FROM uptime_draft_tests WHERE run_id = ?1",
                [existing_run_id],
                |row| row.get(0),
            )?;
            let existing_run = serde_json::from_slice(&payload).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Blob,
                    Box::new(error),
                )
            })?;
            transaction.rollback()?;
            return Ok(DraftTestCreateOutcome::Existing(existing_run));
        }

        let payload = serde_json::to_vec(run)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        transaction.execute(
            "INSERT INTO uptime_draft_tests (run_id, expires_at_unix_seconds, state, payload)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                run.run_id,
                i64::try_from(run.expires_at_unix_seconds).unwrap_or(i64::MAX),
                draft_test_state_name(&run.state),
                payload,
            ],
        )?;
        transaction.execute(
            "INSERT INTO uptime_draft_test_idempotency
             (caller_fingerprint, key_hash, snapshot_hash, run_id, expires_at_unix_seconds)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                caller_fingerprint,
                idempotency_key_hash,
                snapshot_hash,
                run.run_id,
                i64::try_from(run.expires_at_unix_seconds).unwrap_or(i64::MAX),
            ],
        )?;
        transaction.commit()?;
        Ok(DraftTestCreateOutcome::Created(run.clone()))
    }

    pub async fn draft_test(
        &self,
        run_id: &str,
        now: u64,
    ) -> Result<Option<DraftClusterTest>, rusqlite::Error> {
        let runtime = self.inner.lock().await;
        let payload: Option<Vec<u8>> = runtime
            .connection
            .query_row(
                "SELECT payload FROM uptime_draft_tests WHERE run_id = ?1",
                [run_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(payload) = payload else {
            return Ok(None);
        };
        let mut run: DraftClusterTest = serde_json::from_slice(&payload).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Blob,
                Box::new(error),
            )
        })?;
        if now >= run.expires_at_unix_seconds {
            run.state = DraftClusterTestState::Interrupted;
            run.observers.clear();
            run.reason = Some("expired".to_owned());
            let payload = serde_json::to_vec(&run)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
            runtime.connection.execute(
                "UPDATE uptime_draft_tests
                 SET state = 'interrupted', payload = ?2
                 WHERE run_id = ?1",
                params![run_id, payload],
            )?;
        }
        Ok(Some(run))
    }

    pub async fn update_draft_test_observer(
        &self,
        run_id: &str,
        update: DraftClusterTestObserverUpdate,
    ) -> Result<(), rusqlite::Error> {
        let runtime = self.inner.lock().await;
        let payload: Vec<u8> = runtime.connection.query_row(
            "SELECT payload FROM uptime_draft_tests WHERE run_id = ?1",
            [run_id],
            |row| row.get(0),
        )?;
        let mut run: DraftClusterTest = serde_json::from_slice(&payload).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Blob,
                Box::new(error),
            )
        })?;
        if matches!(run.state, DraftClusterTestState::Interrupted) {
            return Ok(());
        }
        if let Some(observer) = run
            .observers
            .iter_mut()
            .find(|observer| observer.node_id == update.node_id)
        {
            observer.state = update.state;
            observer.latency_ms = update.latency_ms;
            observer.status_code = update.status_code;
            observer.error = update.error;
            if observer.started_at_unix_seconds.is_none() {
                observer.started_at_unix_seconds = Some(update.observed_at_unix_seconds);
            }
            observer.completed_at_unix_seconds = Some(update.observed_at_unix_seconds);
        }
        if run.observers.iter().any(|observer| {
            matches!(
                observer.state,
                DraftClusterTestState::Running | DraftClusterTestState::Queued
            )
        }) {
            run.state = DraftClusterTestState::Running;
        } else if run
            .observers
            .iter()
            .any(|observer| matches!(observer.state, DraftClusterTestState::Failed))
        {
            run.state = DraftClusterTestState::Failed;
        } else if run
            .observers
            .iter()
            .all(|observer| matches!(observer.state, DraftClusterTestState::Unsupported))
        {
            run.state = DraftClusterTestState::Unsupported;
        } else {
            run.state = DraftClusterTestState::Succeeded;
        }
        let payload = serde_json::to_vec(&run)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        runtime.connection.execute(
            "UPDATE uptime_draft_tests SET state = ?2, payload = ?3 WHERE run_id = ?1",
            params![run_id, draft_test_state_name(&run.state), payload],
        )?;
        Ok(())
    }

    pub async fn interrupt_draft_test(
        &self,
        run_id: &str,
        reason: &str,
    ) -> Result<(), rusqlite::Error> {
        let runtime = self.inner.lock().await;
        let payload: Vec<u8> = runtime.connection.query_row(
            "SELECT payload FROM uptime_draft_tests WHERE run_id = ?1",
            [run_id],
            |row| row.get(0),
        )?;
        let mut run: DraftClusterTest = serde_json::from_slice(&payload).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Blob,
                Box::new(error),
            )
        })?;
        run.state = DraftClusterTestState::Interrupted;
        run.observers.clear();
        run.reason = Some(reason.to_owned());
        let payload = serde_json::to_vec(&run)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        runtime.connection.execute(
            "UPDATE uptime_draft_tests SET state = 'interrupted', payload = ?2 WHERE run_id = ?1",
            params![run_id, payload],
        )?;
        Ok(())
    }
}

fn cleanup_expired_draft_tests(
    connection: &Connection,
    now_unix_seconds: u64,
) -> Result<(), rusqlite::Error> {
    let tombstone_cutoff = now_unix_seconds.saturating_sub(DRAFT_TEST_TOMBSTONE_TTL_SECONDS);
    connection.execute(
        "DELETE FROM uptime_draft_test_idempotency WHERE expires_at_unix_seconds <= ?1",
        [i64::try_from(now_unix_seconds).unwrap_or(i64::MAX)],
    )?;
    connection.execute(
        "DELETE FROM uptime_draft_tests
         WHERE state = 'interrupted' AND expires_at_unix_seconds <= ?1",
        [i64::try_from(tombstone_cutoff).unwrap_or(i64::MAX)],
    )?;
    connection.execute(
        "DELETE FROM uptime_draft_tests
         WHERE state = 'interrupted'
           AND rowid NOT IN (
               SELECT rowid FROM uptime_draft_tests
               WHERE state = 'interrupted'
               ORDER BY expires_at_unix_seconds DESC
               LIMIT ?1
           )",
        [MAX_DRAFT_TEST_TOMBSTONES],
    )?;
    Ok(())
}

fn draft_test_state_name(state: &DraftClusterTestState) -> &'static str {
    match state {
        DraftClusterTestState::Queued => "queued",
        DraftClusterTestState::Running => "running",
        DraftClusterTestState::Succeeded => "succeeded",
        DraftClusterTestState::Failed => "failed",
        DraftClusterTestState::Unsupported => "unsupported",
        DraftClusterTestState::Interrupted => "interrupted",
    }
}
