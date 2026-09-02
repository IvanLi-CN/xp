use rusqlite::{OptionalExtension, params};

use super::UptimeHandle;

pub const DRAFT_TEST_TTL_SECONDS: u64 = 15 * 60;

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
