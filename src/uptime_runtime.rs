use std::{collections::BTreeMap, net::SocketAddr, path::Path, sync::Arc, time::Instant};

use futures_util::StreamExt as _;
use rusqlite::{Connection, OptionalExtension, params};
use tokio::{
    net::TcpStream,
    sync::{Mutex, OwnedSemaphorePermit, Semaphore},
    time::Duration,
};

use crate::{
    id::new_ulid_string,
    uptime_monitor::{
        DEFAULT_CONNECT_TIMEOUT_SECONDS, DEFAULT_TOTAL_TIMEOUT_SECONDS, HttpMethod,
        MAX_MONITOR_TIMEOUT_SECONDS, MonitorTarget, Observation, ObservationError,
        ObservationOutcome, ServiceMonitor, is_public_ip, normalized_observer_set,
    },
};

const MAX_PENDING_OBSERVATIONS: u64 = 100_000;
const MAX_PENDING_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PENDING_CAPTURE_GAPS: u64 = 100_000;
const HIGH_WATERMARK_PERCENT: u64 = 80;
const LOW_WATERMARK_PERCENT: u64 = 60;
const MAX_REDIRECTS: usize = 3;
const BODY_LIMIT_BYTES: usize = 64 * 1024;
const AD_HOC_CONCURRENCY: usize = 4;
const AD_HOC_RUNS_PER_MINUTE: u8 = 10;

mod capture_gaps;
mod scheduler;
pub use capture_gaps::PendingCaptureGap;
pub(crate) use scheduler::spawn_uptime_worker;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
pub struct PendingObservation {
    pub id: String,
    pub observation: Observation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct CaptureState {
    pub suspended: bool,
    pub pending_observations: u64,
    pub pending_bytes: u64,
}

#[derive(Clone)]
pub struct UptimeHandle {
    inner: Arc<Mutex<UptimeRuntime>>,
    ad_hoc_permits: Arc<Semaphore>,
}

struct UptimeRuntime {
    connection: Connection,
    capture_suspended: bool,
    capture_gaps_suspended: bool,
    ad_hoc_runs: BTreeMap<String, AdHocRateLimit>,
}

#[derive(Debug, Clone, Copy)]
struct AdHocRateLimit {
    minute: u64,
    runs: u8,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdHocRunState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Rejected,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AdHocRun {
    pub run_id: String,
    pub monitor_id: String,
    pub state: AdHocRunState,
    pub created_at_unix_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at_unix_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation: Option<Observation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl UptimeHandle {
    pub fn load(data_dir: &Path) -> Result<Self, rusqlite::Error> {
        let connection = Connection::open(data_dir.join("uptime.sqlite3"))?;
        connection.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS uptime_observations (
                id TEXT PRIMARY KEY,
                monitor_id TEXT NOT NULL,
                revision INTEGER NOT NULL,
                observer_node_id TEXT NOT NULL,
                slot_unix_seconds INTEGER NOT NULL,
                observed_at_unix_seconds INTEGER NOT NULL,
                ad_hoc INTEGER NOT NULL,
                enqueued INTEGER NOT NULL DEFAULT 0,
                payload BLOB NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS uptime_observations_scheduled_slot
                ON uptime_observations
                (monitor_id, revision, observer_node_id, slot_unix_seconds)
                WHERE ad_hoc = 0;
            CREATE INDEX IF NOT EXISTS uptime_observations_pending
                ON uptime_observations (enqueued, observed_at_unix_seconds, id);
            CREATE INDEX IF NOT EXISTS uptime_observations_monitor_history
                ON uptime_observations (monitor_id, observed_at_unix_seconds, id);
            CREATE TABLE IF NOT EXISTS uptime_capture_gaps (
                id TEXT PRIMARY KEY,
                monitor_id TEXT NOT NULL,
                revision INTEGER NOT NULL,
                observer_node_id TEXT NOT NULL,
                interval_seconds INTEGER NOT NULL,
                observer_set_node_ids_json BLOB NOT NULL,
                start_slot_unix_seconds INTEGER NOT NULL,
                end_slot_unix_seconds INTEGER NOT NULL,
                enqueued INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS uptime_capture_gaps_pending
                ON uptime_capture_gaps (enqueued, end_slot_unix_seconds, id);
            CREATE INDEX IF NOT EXISTS uptime_capture_gaps_coalesce
                ON uptime_capture_gaps
                (monitor_id, revision, observer_node_id, interval_seconds, enqueued,
                 end_slot_unix_seconds);
            CREATE TABLE IF NOT EXISTS uptime_runs (
                run_id TEXT PRIMARY KEY,
                monitor_id TEXT NOT NULL,
                state TEXT NOT NULL,
                created_at_unix_seconds INTEGER NOT NULL,
                completed_at_unix_seconds INTEGER,
                observation_id TEXT,
                reason TEXT
            );
            CREATE INDEX IF NOT EXISTS uptime_runs_monitor
                ON uptime_runs (monitor_id, created_at_unix_seconds DESC);
            ",
        )?;
        migrate_observation_idempotency(&connection)?;
        let mut runtime = UptimeRuntime {
            connection,
            capture_suspended: false,
            capture_gaps_suspended: false,
            ad_hoc_runs: BTreeMap::new(),
        };
        runtime.refresh_capture_state()?;
        Ok(Self {
            inner: Arc::new(Mutex::new(runtime)),
            ad_hoc_permits: Arc::new(Semaphore::new(AD_HOC_CONCURRENCY)),
        })
    }

    pub async fn record(&self, observation: Observation) -> Result<(), rusqlite::Error> {
        let _ = self.record_with_id(new_ulid_string(), observation).await?;
        Ok(())
    }

    pub async fn record_with_id(
        &self,
        id: String,
        observation: Observation,
    ) -> Result<bool, rusqlite::Error> {
        let mut runtime = self.inner.lock().await;
        runtime.refresh_capture_state()?;
        if runtime.capture_suspended || runtime.capture_gaps_suspended {
            return Ok(false);
        }
        let payload = serde_json::to_vec(&observation)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let changed = runtime.connection.execute(
            "INSERT OR IGNORE INTO uptime_observations
             (id, monitor_id, revision, observer_node_id, slot_unix_seconds,
              observed_at_unix_seconds, ad_hoc, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                observation.monitor_id,
                i64::try_from(observation.revision).unwrap_or(i64::MAX),
                observation.observer_node_id,
                i64::try_from(observation.slot_unix_seconds).unwrap_or(i64::MAX),
                i64::try_from(observation.observed_at_unix_seconds).unwrap_or(i64::MAX),
                observation.ad_hoc,
                payload,
            ],
        )? == 1;
        runtime.refresh_capture_state()?;
        Ok(changed)
    }

    pub async fn acquire_ad_hoc(
        &self,
        now_unix_seconds: u64,
        token_fingerprint: &str,
    ) -> Option<OwnedSemaphorePermit> {
        let permit = self.ad_hoc_permits.clone().try_acquire_owned().ok()?;
        let mut runtime = self.inner.lock().await;
        let minute = now_unix_seconds / 60;
        runtime
            .ad_hoc_runs
            .retain(|_, limiter| limiter.minute == minute);
        let limiter = runtime
            .ad_hoc_runs
            .entry(token_fingerprint.to_owned())
            .or_insert(AdHocRateLimit { minute, runs: 0 });
        if limiter.runs >= AD_HOC_RUNS_PER_MINUTE {
            return None;
        }
        limiter.runs = limiter.runs.saturating_add(1);
        Some(permit)
    }

    pub async fn create_ad_hoc_run(&self, run: &AdHocRun) -> Result<(), rusqlite::Error> {
        let runtime = self.inner.lock().await;
        runtime.connection.execute(
            "INSERT INTO uptime_runs
             (run_id, monitor_id, state, created_at_unix_seconds, completed_at_unix_seconds,
              observation_id, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)",
            params![
                run.run_id,
                run.monitor_id,
                ad_hoc_run_state_name(&run.state),
                i64::try_from(run.created_at_unix_seconds).unwrap_or(i64::MAX),
                run.completed_at_unix_seconds
                    .map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
                run.reason,
            ],
        )?;
        Ok(())
    }

    pub async fn mark_ad_hoc_run_running(&self, run_id: &str) -> Result<(), rusqlite::Error> {
        let runtime = self.inner.lock().await;
        runtime.connection.execute(
            "UPDATE uptime_runs SET state = 'running' WHERE run_id = ?1",
            [run_id],
        )?;
        Ok(())
    }

    pub async fn complete_ad_hoc_run(
        &self,
        run_id: &str,
        observation: &Observation,
    ) -> Result<(), rusqlite::Error> {
        let runtime = self.inner.lock().await;
        let state = match observation.outcome {
            ObservationOutcome::Success => "succeeded",
            ObservationOutcome::Failure
            | ObservationOutcome::Unsupported
            | ObservationOutcome::Suspended => "failed",
        };
        runtime.connection.execute(
            "UPDATE uptime_runs
             SET state = ?2, completed_at_unix_seconds = ?3, observation_id = ?1
             WHERE run_id = ?1",
            params![
                run_id,
                state,
                i64::try_from(observation.observed_at_unix_seconds).unwrap_or(i64::MAX),
            ],
        )?;
        Ok(())
    }

    pub async fn reject_ad_hoc_run(
        &self,
        run_id: &str,
        now_unix_seconds: u64,
        reason: &str,
    ) -> Result<(), rusqlite::Error> {
        let runtime = self.inner.lock().await;
        runtime.connection.execute(
            "UPDATE uptime_runs
             SET state = 'rejected', completed_at_unix_seconds = ?2, reason = ?3
             WHERE run_id = ?1",
            params![
                run_id,
                i64::try_from(now_unix_seconds).unwrap_or(i64::MAX),
                reason,
            ],
        )?;
        Ok(())
    }

    pub async fn ad_hoc_run(&self, run_id: &str) -> Result<Option<AdHocRun>, rusqlite::Error> {
        let runtime = self.inner.lock().await;
        runtime
            .connection
            .query_row(
                "SELECT run_id, monitor_id, state, created_at_unix_seconds,
                        completed_at_unix_seconds, observation_id, reason
                 FROM uptime_runs WHERE run_id = ?1",
                [run_id],
                |row| {
                    let observation_id: Option<String> = row.get(5)?;
                    let observation = observation_id
                        .as_deref()
                        .map(|id| {
                            runtime.connection.query_row(
                                "SELECT payload FROM uptime_observations WHERE id = ?1",
                                [id],
                                |observation_row| {
                                    let payload: Vec<u8> = observation_row.get(0)?;
                                    serde_json::from_slice(&payload).map_err(|error| {
                                        rusqlite::Error::FromSqlConversionFailure(
                                            0,
                                            rusqlite::types::Type::Blob,
                                            Box::new(error),
                                        )
                                    })
                                },
                            )
                        })
                        .transpose()?;
                    Ok(AdHocRun {
                        run_id: row.get(0)?,
                        monitor_id: row.get(1)?,
                        state: ad_hoc_run_state_from_name(&row.get::<_, String>(2)?)?,
                        created_at_unix_seconds: u64::try_from(row.get::<_, i64>(3)?)
                            .unwrap_or_default(),
                        completed_at_unix_seconds: row
                            .get::<_, Option<i64>>(4)?
                            .map(|value| u64::try_from(value).unwrap_or_default()),
                        observation,
                        reason: row.get(6)?,
                    })
                },
            )
            .optional()
    }

    pub async fn capture_state(&self) -> Result<CaptureState, rusqlite::Error> {
        let mut runtime = self.inner.lock().await;
        runtime.refresh_capture_state()?;
        runtime.capture_state()
    }

    pub async fn pending(&self, limit: usize) -> Result<Vec<PendingObservation>, rusqlite::Error> {
        let runtime = self.inner.lock().await;
        let mut statement = runtime.connection.prepare(
            "SELECT id, payload FROM uptime_observations
             WHERE enqueued = 0
             ORDER BY observed_at_unix_seconds, id
             LIMIT ?1",
        )?;
        let rows = statement.query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
            let payload: Vec<u8> = row.get(1)?;
            let observation = serde_json::from_slice(&payload).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Blob,
                    Box::new(error),
                )
            })?;
            Ok(PendingObservation {
                id: row.get(0)?,
                observation,
            })
        })?;
        rows.collect()
    }

    pub async fn mark_enqueued(&self, ids: &[String]) -> Result<(), rusqlite::Error> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut runtime = self.inner.lock().await;
        let transaction = runtime.connection.transaction()?;
        for id in ids {
            transaction.execute(
                "UPDATE uptime_observations SET enqueued = 1 WHERE id = ?1",
                [id],
            )?;
        }
        transaction.commit()?;
        runtime.refresh_capture_state()
    }

    pub async fn observations(
        &self,
        monitor_id: &str,
        start_unix_seconds: u64,
        end_unix_seconds: u64,
        limit: usize,
    ) -> Result<Vec<Observation>, rusqlite::Error> {
        let runtime = self.inner.lock().await;
        let mut statement = runtime.connection.prepare(
            "SELECT payload FROM uptime_observations
             WHERE monitor_id = ?1
               AND observed_at_unix_seconds >= ?2
               AND observed_at_unix_seconds <= ?3
             ORDER BY observed_at_unix_seconds, id
             LIMIT ?4",
        )?;
        let rows = statement.query_map(
            params![
                monitor_id,
                i64::try_from(start_unix_seconds).unwrap_or(i64::MAX),
                i64::try_from(end_unix_seconds).unwrap_or(i64::MAX),
                i64::try_from(limit).unwrap_or(i64::MAX),
            ],
            |row| {
                let payload: Vec<u8> = row.get(0)?;
                serde_json::from_slice(&payload).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Blob,
                        Box::new(error),
                    )
                })
            },
        )?;
        rows.collect()
    }

    pub async fn latest(&self, monitor_id: &str) -> Result<Option<Observation>, rusqlite::Error> {
        let runtime = self.inner.lock().await;
        runtime
            .connection
            .query_row(
                "SELECT payload FROM uptime_observations
                 WHERE monitor_id = ?1 ORDER BY observed_at_unix_seconds DESC, id DESC LIMIT 1",
                [monitor_id],
                |row| {
                    let payload: Vec<u8> = row.get(0)?;
                    serde_json::from_slice(&payload).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Blob,
                            Box::new(error),
                        )
                    })
                },
            )
            .optional()
    }

    pub async fn prune_enqueued_before(
        &self,
        before_unix_seconds: u64,
    ) -> Result<(), rusqlite::Error> {
        let runtime = self.inner.lock().await;
        runtime.connection.execute(
            "DELETE FROM uptime_observations
             WHERE enqueued = 1 AND observed_at_unix_seconds < ?1",
            [i64::try_from(before_unix_seconds).unwrap_or(i64::MAX)],
        )?;
        runtime.connection.execute(
            "DELETE FROM uptime_capture_gaps
             WHERE enqueued = 1 AND end_slot_unix_seconds < ?1",
            [i64::try_from(before_unix_seconds).unwrap_or(i64::MAX)],
        )?;
        Ok(())
    }

    pub async fn run(
        &self,
        monitor: &ServiceMonitor,
        observer_node_id: String,
        observer_set_node_ids: Vec<String>,
        slot_unix_seconds: u64,
        ad_hoc: bool,
    ) -> Observation {
        let observed_at_unix_seconds =
            u64::try_from(chrono::Utc::now().timestamp()).unwrap_or_default();
        let result = execute_target(&monitor.target).await;
        let (outcome, error, latency_ms, status_code, packet_loss_percent) = match result {
            Ok(result) => (
                ObservationOutcome::Success,
                None,
                Some(result.latency_ms),
                result.status_code,
                result.packet_loss_percent,
            ),
            Err(error) if error == ObservationError::IcmpUnsupported => {
                (ObservationOutcome::Unsupported, Some(error), None, None, 0)
            }
            Err(error) => (ObservationOutcome::Failure, Some(error), None, None, 0),
        };
        let observer_set_node_ids = normalized_observer_set(&observer_set_node_ids)
            .unwrap_or_else(|| vec![observer_node_id.clone()]);
        Observation {
            monitor_id: monitor.monitor_id.clone(),
            revision: monitor.revision,
            observer_node_id,
            expected_observer_count: u32::try_from(observer_set_node_ids.len())
                .unwrap_or(u32::MAX)
                .max(1),
            observer_set_node_ids,
            slot_unix_seconds,
            observed_at_unix_seconds,
            outcome,
            error,
            latency_ms,
            status_code,
            packet_loss_percent,
            ad_hoc,
        }
    }
}

fn migrate_observation_idempotency(connection: &Connection) -> Result<(), rusqlite::Error> {
    let schema: Option<String> = connection
        .query_row(
            "SELECT sql FROM sqlite_master
             WHERE type = 'table' AND name = 'uptime_observations'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let Some(schema) = schema else {
        return Ok(());
    };
    if !schema
        .contains("UNIQUE (monitor_id, revision, observer_node_id, slot_unix_seconds, ad_hoc)")
    {
        return Ok(());
    }

    connection.execute_batch(
        "
        BEGIN IMMEDIATE;
        CREATE TABLE uptime_observations_rebuilt (
            id TEXT PRIMARY KEY,
            monitor_id TEXT NOT NULL,
            revision INTEGER NOT NULL,
            observer_node_id TEXT NOT NULL,
            slot_unix_seconds INTEGER NOT NULL,
            observed_at_unix_seconds INTEGER NOT NULL,
            ad_hoc INTEGER NOT NULL,
            enqueued INTEGER NOT NULL DEFAULT 0,
            payload BLOB NOT NULL
        );
        INSERT INTO uptime_observations_rebuilt
            SELECT id, monitor_id, revision, observer_node_id, slot_unix_seconds,
                   observed_at_unix_seconds, ad_hoc, enqueued, payload
            FROM uptime_observations;
        DROP TABLE uptime_observations;
        ALTER TABLE uptime_observations_rebuilt RENAME TO uptime_observations;
        CREATE UNIQUE INDEX uptime_observations_scheduled_slot
            ON uptime_observations
            (monitor_id, revision, observer_node_id, slot_unix_seconds)
            WHERE ad_hoc = 0;
        CREATE INDEX uptime_observations_pending
            ON uptime_observations (enqueued, observed_at_unix_seconds, id);
        CREATE INDEX uptime_observations_monitor_history
            ON uptime_observations (monitor_id, observed_at_unix_seconds, id);
        COMMIT;
        ",
    )
}

impl UptimeRuntime {
    fn capture_state(&self) -> Result<CaptureState, rusqlite::Error> {
        let (count, bytes): (i64, i64) = self.connection.query_row(
            "SELECT COUNT(*), COALESCE(SUM(length(payload)), 0)
             FROM uptime_observations WHERE enqueued = 0",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(CaptureState {
            suspended: self.capture_suspended || self.capture_gaps_suspended,
            pending_observations: u64::try_from(count).unwrap_or(u64::MAX),
            pending_bytes: u64::try_from(bytes).unwrap_or(u64::MAX),
        })
    }

    fn refresh_capture_state(&mut self) -> Result<(), rusqlite::Error> {
        let state = self.capture_state()?;
        let high_count = MAX_PENDING_OBSERVATIONS * HIGH_WATERMARK_PERCENT / 100;
        let high_bytes = MAX_PENDING_BYTES * HIGH_WATERMARK_PERCENT / 100;
        let low_count = MAX_PENDING_OBSERVATIONS * LOW_WATERMARK_PERCENT / 100;
        let low_bytes = MAX_PENDING_BYTES * LOW_WATERMARK_PERCENT / 100;
        if self.capture_suspended {
            self.capture_suspended =
                state.pending_observations > low_count || state.pending_bytes > low_bytes;
        } else {
            self.capture_suspended =
                state.pending_observations >= high_count || state.pending_bytes >= high_bytes;
        }
        let pending_capture_gaps = self.pending_capture_gap_count()?;
        let high_gaps = MAX_PENDING_CAPTURE_GAPS * HIGH_WATERMARK_PERCENT / 100;
        let low_gaps = MAX_PENDING_CAPTURE_GAPS * LOW_WATERMARK_PERCENT / 100;
        self.capture_gaps_suspended = if self.capture_gaps_suspended {
            pending_capture_gaps > low_gaps
        } else {
            pending_capture_gaps >= high_gaps
        };
        Ok(())
    }
}

fn ad_hoc_run_state_name(state: &AdHocRunState) -> &'static str {
    match state {
        AdHocRunState::Queued => "queued",
        AdHocRunState::Running => "running",
        AdHocRunState::Succeeded => "succeeded",
        AdHocRunState::Failed => "failed",
        AdHocRunState::Rejected => "rejected",
    }
}

fn ad_hoc_run_state_from_name(name: &str) -> Result<AdHocRunState, rusqlite::Error> {
    match name {
        "queued" => Ok(AdHocRunState::Queued),
        "running" => Ok(AdHocRunState::Running),
        "succeeded" => Ok(AdHocRunState::Succeeded),
        "failed" => Ok(AdHocRunState::Failed),
        "rejected" => Ok(AdHocRunState::Rejected),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

struct ExecutionResult {
    latency_ms: u32,
    status_code: Option<u16>,
    packet_loss_percent: u8,
}

async fn execute_target(target: &MonitorTarget) -> Result<ExecutionResult, ObservationError> {
    tokio::time::timeout(
        Duration::from_secs(u64::from(MAX_MONITOR_TIMEOUT_SECONDS)),
        execute_target_with_total_timeout(target),
    )
    .await
    .map_err(|_| ObservationError::TotalTimeout)?
}

async fn execute_target_with_total_timeout(
    target: &MonitorTarget,
) -> Result<ExecutionResult, ObservationError> {
    tokio::time::timeout(
        Duration::from_secs(u64::from(DEFAULT_TOTAL_TIMEOUT_SECONDS)),
        execute_target_inner(target),
    )
    .await
    .map_err(|_| ObservationError::TotalTimeout)?
}

async fn execute_target_inner(target: &MonitorTarget) -> Result<ExecutionResult, ObservationError> {
    match target {
        MonitorTarget::Http {
            url,
            method,
            accepted_statuses,
            body_contains,
        } => {
            execute_http(
                url,
                method,
                accepted_statuses,
                body_contains.as_deref(),
                false,
            )
            .await
        }
        MonitorTarget::Https {
            url,
            method,
            accepted_statuses,
            body_contains,
        } => {
            execute_http(
                url,
                method,
                accepted_statuses,
                body_contains.as_deref(),
                true,
            )
            .await
        }
        MonitorTarget::Tcping { host, port } => execute_tcp(host, *port).await,
        MonitorTarget::Ping { host } => execute_ping(host).await,
    }
}

async fn execute_http(
    target_url: &str,
    method: &HttpMethod,
    accepted_statuses: &[crate::uptime_monitor::StatusRange],
    body_contains: Option<&str>,
    requires_https: bool,
) -> Result<ExecutionResult, ObservationError> {
    let mut url = reqwest::Url::parse(target_url).map_err(|_| ObservationError::TargetBlocked)?;
    let started = Instant::now();
    for redirects in 0..=MAX_REDIRECTS {
        let host = url.host_str().ok_or(ObservationError::TargetBlocked)?;
        let port = url
            .port_or_known_default()
            .ok_or(ObservationError::TargetBlocked)?;
        let addresses = resolve_public_addresses(host, port).await?;
        let remaining = Duration::from_secs(u64::from(DEFAULT_TOTAL_TIMEOUT_SECONDS))
            .checked_sub(started.elapsed())
            .ok_or(ObservationError::TotalTimeout)?;
        let mut builder = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(u64::from(
                DEFAULT_CONNECT_TIMEOUT_SECONDS,
            )))
            .timeout(remaining)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy();
        for address in addresses {
            builder = builder.resolve(host, address);
        }
        let client = builder.build().map_err(|_| ObservationError::Internal)?;
        let request = match method {
            HttpMethod::Get => client.get(url.clone()),
            HttpMethod::Head => client.head(url.clone()),
        };
        let response = tokio::time::timeout(remaining, request.send())
            .await
            .map_err(|_| ObservationError::TotalTimeout)?
            .map_err(|error| {
                if error.is_timeout() {
                    ObservationError::ConnectTimeout
                } else {
                    ObservationError::TcpConnect
                }
            })?;
        if response.status().is_redirection() {
            if redirects == MAX_REDIRECTS {
                return Err(ObservationError::RedirectBlocked);
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(ObservationError::RedirectBlocked)?;
            url = url
                .join(location)
                .map_err(|_| ObservationError::RedirectBlocked)?;
            if !matches!(url.scheme(), "http" | "https")
                || (requires_https && url.scheme() != "https")
            {
                return Err(ObservationError::RedirectBlocked);
            }
            continue;
        }
        let status = response.status().as_u16();
        if !accepted_statuses.iter().any(|range| range.contains(status)) {
            return Err(ObservationError::HttpStatus);
        }
        if let Some(needle) = body_contains {
            let mut body = Vec::new();
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|_| ObservationError::TcpConnect)?;
                let remaining = BODY_LIMIT_BYTES.saturating_sub(body.len());
                body.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
                if body.len() == BODY_LIMIT_BYTES {
                    break;
                }
            }
            if !body
                .windows(needle.len())
                .any(|window| window == needle.as_bytes())
            {
                return Err(ObservationError::BodyMismatch);
            }
        }
        return Ok(ExecutionResult {
            latency_ms: duration_ms(started.elapsed()),
            status_code: Some(status),
            packet_loss_percent: 0,
        });
    }
    Err(ObservationError::RedirectBlocked)
}

async fn execute_tcp(host: &str, port: u16) -> Result<ExecutionResult, ObservationError> {
    let started = Instant::now();
    let addresses = resolve_public_addresses(host, port).await?;
    for address in addresses {
        let Some(timeout) = tcp_connect_timeout(started.elapsed()) else {
            return Err(ObservationError::TotalTimeout);
        };
        match tokio::time::timeout(timeout, TcpStream::connect(address)).await {
            Ok(Ok(_stream)) => {
                return Ok(ExecutionResult {
                    latency_ms: duration_ms(started.elapsed()),
                    status_code: None,
                    packet_loss_percent: 0,
                });
            }
            Ok(Err(_)) => {}
            Err(_) => return Err(ObservationError::ConnectTimeout),
        }
    }
    Err(ObservationError::TcpConnect)
}

fn tcp_connect_timeout(elapsed: Duration) -> Option<Duration> {
    Duration::from_secs(u64::from(DEFAULT_TOTAL_TIMEOUT_SECONDS))
        .checked_sub(elapsed)
        .filter(|remaining| !remaining.is_zero())
        .map(|remaining| {
            remaining.min(Duration::from_secs(u64::from(
                DEFAULT_CONNECT_TIMEOUT_SECONDS,
            )))
        })
}

async fn execute_ping(host: &str) -> Result<ExecutionResult, ObservationError> {
    let addresses = resolve_public_addresses(host, 0).await?;
    let Some(address) = addresses.iter().find_map(|address| match address {
        SocketAddr::V4(address) => Some(*address),
        SocketAddr::V6(_) => None,
    }) else {
        return Err(ObservationError::IcmpUnsupported);
    };
    let result = tokio::time::timeout(
        Duration::from_secs(u64::from(DEFAULT_TOTAL_TIMEOUT_SECONDS)),
        tokio::task::spawn_blocking(move || send_icmp_echoes(address)),
    )
    .await
    .map_err(|_| ObservationError::TotalTimeout)?
    .map_err(|_| ObservationError::Internal)??;
    Ok(ExecutionResult {
        latency_ms: result.latency_ms,
        status_code: None,
        packet_loss_percent: result.packet_loss_percent,
    })
}

async fn resolve_public_addresses(
    host: &str,
    port: u16,
) -> Result<Vec<SocketAddr>, ObservationError> {
    let addresses = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| ObservationError::Dns)?
        .collect::<Vec<_>>();
    if addresses.is_empty() || addresses.iter().any(|address| !is_public_ip(address.ip())) {
        return Err(ObservationError::TargetBlocked);
    }
    Ok(addresses)
}

struct PingResult {
    latency_ms: u32,
    packet_loss_percent: u8,
}

#[cfg(target_os = "linux")]
fn send_icmp_echoes(address: std::net::SocketAddrV4) -> Result<PingResult, ObservationError> {
    use std::time::Duration as StdDuration;

    let socket = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, libc::IPPROTO_ICMP) };
    let socket = if socket >= 0 {
        socket
    } else {
        unsafe { libc::socket(libc::AF_INET, libc::SOCK_RAW, libc::IPPROTO_ICMP) }
    };
    if socket < 0 {
        return Err(ObservationError::IcmpUnsupported);
    }
    let timeout = libc::timeval {
        tv_sec: 2,
        tv_usec: 0,
    };
    unsafe {
        libc::setsockopt(
            socket,
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            (&timeout as *const libc::timeval).cast(),
            std::mem::size_of::<libc::timeval>() as libc::socklen_t,
        );
    }
    let mut successful = 0_u8;
    let mut latency_total = StdDuration::ZERO;
    for sequence in 0_u16..3 {
        let mut packet = [8_u8, 0, 0, 0, 0, 0, (sequence >> 8) as u8, sequence as u8];
        let checksum = icmp_checksum(&packet);
        packet[2] = (checksum >> 8) as u8;
        packet[3] = checksum as u8;
        let destination = libc::sockaddr_in {
            sin_family: libc::AF_INET as libc::sa_family_t,
            sin_port: 0,
            sin_addr: libc::in_addr {
                s_addr: u32::from_ne_bytes(address.ip().octets()),
            },
            sin_zero: [0; 8],
        };
        let started = Instant::now();
        let sent = unsafe {
            libc::sendto(
                socket,
                packet.as_ptr().cast(),
                packet.len(),
                0,
                (&destination as *const libc::sockaddr_in).cast(),
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            )
        };
        if sent < 0 {
            continue;
        }
        let mut response = [0_u8; 512];
        let received =
            unsafe { libc::recv(socket, response.as_mut_ptr().cast(), response.len(), 0) };
        let response = &response[..usize::try_from(received).unwrap_or_default()];
        if received > 0 && is_icmp_echo_reply(response, sequence) {
            successful = successful.saturating_add(1);
            latency_total = latency_total.saturating_add(started.elapsed());
        }
    }
    unsafe {
        libc::close(socket);
    }
    if successful == 0 {
        return Err(ObservationError::IcmpTimeout);
    }
    Ok(PingResult {
        latency_ms: duration_ms(latency_total / u32::from(successful)),
        packet_loss_percent: (3_u8.saturating_sub(successful)).saturating_mul(100) / 3,
    })
}

#[cfg(target_os = "linux")]
fn is_icmp_echo_reply(packet: &[u8], sequence: u16) -> bool {
    let offset = if packet
        .first()
        .is_some_and(|first| first >> 4 == 4 && packet.len() >= 20)
    {
        usize::from(packet[0] & 0x0f) * 4
    } else {
        0
    };
    packet
        .get(offset..)
        .is_some_and(|icmp| icmp.len() >= 8 && icmp[0] == 0 && icmp[1] == 0)
        && packet
            .get(offset + 6..offset + 8)
            .is_some_and(|bytes| bytes == sequence.to_be_bytes())
}

pub fn icmp_supported() -> bool {
    #[cfg(target_os = "linux")]
    {
        let socket = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, libc::IPPROTO_ICMP) };
        let socket = if socket >= 0 {
            socket
        } else {
            unsafe { libc::socket(libc::AF_INET, libc::SOCK_RAW, libc::IPPROTO_ICMP) }
        };
        if socket < 0 {
            return false;
        }
        unsafe {
            libc::close(socket);
        }
        true
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

#[cfg(not(target_os = "linux"))]
fn send_icmp_echoes(_address: std::net::SocketAddrV4) -> Result<PingResult, ObservationError> {
    Err(ObservationError::IcmpUnsupported)
}

#[cfg(target_os = "linux")]
fn icmp_checksum(packet: &[u8]) -> u16 {
    let mut sum = 0_u32;
    for chunk in packet.chunks(2) {
        let word = if chunk.len() == 2 {
            u16::from_be_bytes([chunk[0], chunk[1]])
        } else {
            u16::from_be_bytes([chunk[0], 0])
        };
        sum = sum.saturating_add(u32::from(word));
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff).saturating_add(sum >> 16);
    }
    !(sum as u16)
}

fn duration_ms(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX)
}
