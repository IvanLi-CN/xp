use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tracing::warn;
use uuid::Uuid;

use crate::cluster_metadata::write_atomic_private;

use super::HistoryStorage;

pub(crate) const HISTORY_STORAGE_DIAGNOSTICS_FILE: &str = "history.sqlite3.diagnostics.json";

const DIAGNOSTIC_SCHEMA_VERSION: u32 = 1;
const DIAGNOSTIC_MAX_BYTES: usize = 16 * 1024;
const DIAGNOSTIC_PERSIST_INTERVAL: Duration = Duration::from_millis(100);
const DIAGNOSTIC_RETRY_BASE_MILLISECONDS: u64 = 1_000;
const DIAGNOSTIC_RETRY_MAX_MILLISECONDS: u64 = 60_000;
#[cfg(not(test))]
const SLOW_OPERATION_MILLISECONDS: u64 = 1_000;
#[cfg(test)]
const SLOW_OPERATION_MILLISECONDS: u64 = 0;

#[derive(Debug, Clone, Copy)]
pub(crate) enum HistoryStorageDiagnosticOperation {
    RuntimeStatus,
    RuntimeStatusRecordCount,
    RuntimeStatusSegmentCount,
    RepositoryHistoryUsedBytes,
    SourceDeliveryJournalSummary,
    SourceDeliveryJournalState,
    SourceDeliveryJournalOldest,
    TieredBackfillPage,
    TieredBackfillExportRefresh,
    TieredBackfillExportFinish,
    TieredBackfillExportActiveCount,
    TieredBackfillExportUpsert,
    TieredBackfillExportSession,
    TieredBackfillExportWatermarks,
    TieredBackfillReceivedAtCutoff,
    TieredBackfillTombstoneWatermark,
    TieredBackfillRecordWatermark,
    TieredBackfillRecordsPage,
    RetentionActiveExport,
    RetentionPrune,
    RetentionCompactionPage,
    RetentionExpiredRecordProbe,
    RetentionReplaceAndPrune,
}

impl HistoryStorageDiagnosticOperation {
    fn operation_id(self) -> &'static str {
        match self {
            Self::RuntimeStatus => "runtime_status",
            Self::RuntimeStatusRecordCount => "runtime_status.record_count",
            Self::RuntimeStatusSegmentCount => "runtime_status.segment_count",
            Self::RepositoryHistoryUsedBytes => "repository_history.used_bytes",
            Self::SourceDeliveryJournalSummary => "source_delivery.journal_summary",
            Self::SourceDeliveryJournalState => "source_delivery.journal_state",
            Self::SourceDeliveryJournalOldest => "source_delivery.journal_oldest",
            Self::TieredBackfillPage => "tiered_backfill_page",
            Self::TieredBackfillExportRefresh => "tiered_backfill.export_refresh",
            Self::TieredBackfillExportFinish => "tiered_backfill.export_finish",
            Self::TieredBackfillExportActiveCount => "tiered_backfill.export_active_count",
            Self::TieredBackfillExportUpsert => "tiered_backfill.export_upsert",
            Self::TieredBackfillExportSession => "tiered_backfill.export_session",
            Self::TieredBackfillExportWatermarks => "tiered_backfill.export_watermarks",
            Self::TieredBackfillReceivedAtCutoff => {
                "tiered_backfill.export_watermarks.received_at_cutoff"
            }
            Self::TieredBackfillTombstoneWatermark => {
                "tiered_backfill.export_watermarks.tombstone_watermark"
            }
            Self::TieredBackfillRecordWatermark => {
                "tiered_backfill.export_watermarks.record_watermark"
            }
            Self::TieredBackfillRecordsPage => "tiered_backfill.records_page",
            Self::RetentionActiveExport => "retention.active_export",
            Self::RetentionPrune => "retention.prune",
            Self::RetentionCompactionPage => "retention.compaction_page",
            Self::RetentionExpiredRecordProbe => "retention.expired_record_probe",
            Self::RetentionReplaceAndPrune => "retention.replace_and_prune",
        }
    }

    fn statement(self) -> &'static str {
        match self {
            Self::RuntimeStatus => "composite history repository runtime status",
            Self::RuntimeStatusRecordCount => concat!(
                "SELECT COUNT(source_node_id) FROM repository_history_records ",
                "INDEXED BY repository_history_records_keyset"
            ),
            Self::RuntimeStatusSegmentCount => concat!(
                "SELECT COUNT(id) FROM repository_history_segments ",
                "INDEXED BY repository_history_segments_sync_order_v2"
            ),
            Self::RepositoryHistoryUsedBytes => concat!(
                "PRAGMA page_count; PRAGMA page_size; stat ",
                "history.sqlite3-wal"
            ),
            Self::SourceDeliveryJournalSummary => "composite source delivery journal summary",
            Self::SourceDeliveryJournalState => concat!(
                "SELECT pending_segments, pending_bytes, last_acknowledged_at, ",
                "last_delivery_path, order_repair_completed, capacity_suspended ",
                "FROM source_delivery_journal_state WHERE singleton = 1"
            ),
            Self::SourceDeliveryJournalOldest => concat!(
                "SELECT id, stream, closed_at, identity, wire FROM source_delivery_journal ",
                "ORDER BY (stream = 'tombstone') DESC, source_node_id, source_epoch, ",
                "stream, first_sequence, created_at, id LIMIT 1"
            ),
            Self::TieredBackfillPage => "composite tiered history backfill page",
            Self::TieredBackfillExportRefresh => "composite tiered backfill export refresh",
            Self::TieredBackfillExportFinish => {
                "DELETE FROM repository_history_export_leases WHERE session_id = ?1"
            }
            Self::TieredBackfillExportActiveCount => {
                "SELECT COUNT(*) FROM repository_history_export_leases"
            }
            Self::TieredBackfillExportUpsert => concat!(
                "INSERT INTO repository_history_export_leases (session_id, expires_at) ",
                "VALUES (?1, ?2) ON CONFLICT(session_id) DO UPDATE SET ",
                "expires_at = excluded.expires_at"
            ),
            Self::TieredBackfillExportSession => concat!(
                "DELETE FROM repository_history_export_leases WHERE expires_at <= ?1; ",
                "SELECT EXISTS(SELECT 1 FROM repository_history_export_leases ",
                "WHERE session_id = ?1)"
            ),
            Self::TieredBackfillExportWatermarks => "composite tiered backfill export watermarks",
            Self::TieredBackfillReceivedAtCutoff => concat!(
                "SELECT MAX(received_at) FROM repository_history_records ",
                "WHERE (is_tombstone = 1 OR observed_end < ?1)"
            ),
            Self::TieredBackfillTombstoneWatermark | Self::TieredBackfillRecordWatermark => {
                concat!(
                    "SELECT source_node_id, source_epoch, stream, sequence, observed_start ",
                    "FROM repository_history_records WHERE is_tombstone = ?1 ",
                    "AND (is_tombstone = 1 OR observed_end < ?2) AND received_at <= ?3 ",
                    "ORDER BY observed_start DESC, source_node_id DESC, source_epoch DESC, ",
                    "stream DESC, sequence DESC LIMIT 1"
                )
            }
            Self::TieredBackfillRecordsPage => concat!(
                "SELECT source_node_id, source_epoch, stream, sequence, subject_node_id, ",
                "observer_node_id, schema_id, schema_version, record_key, is_tombstone, ",
                "observed_start, observed_end, received_at, payload FROM ",
                "repository_history_records INDEXED BY repository_history_records_keyset ",
                "WHERE is_tombstone = ?1 AND (is_tombstone = 1 OR observed_end < ?2) ",
                "AND received_at <= ?3 AND (observed_start, source_node_id, source_epoch, ",
                "stream, sequence) > (?4, ?5, ?6, ?7, ?8) AND (observed_start, ",
                "source_node_id, source_epoch, stream, sequence) <= (?9, ?10, ?11, ?12, ?13) ",
                "ORDER BY observed_start, source_node_id, source_epoch, stream, sequence ",
                "LIMIT ?14"
            ),
            Self::RetentionActiveExport => concat!(
                "DELETE FROM repository_history_export_leases WHERE expires_at <= ?1; ",
                "SELECT EXISTS(SELECT 1 FROM repository_history_export_leases)"
            ),
            Self::RetentionPrune => "composite retention prune",
            Self::RetentionCompactionPage => concat!(
                "SELECT source_node_id, source_epoch, stream, sequence, subject_node_id, ",
                "observer_node_id, schema_id, schema_version, record_key, is_tombstone, ",
                "observed_start, observed_end, received_at, payload, aggregate_complete, ",
                "aggregate_start, aggregate_end FROM repository_history_records INDEXED BY ",
                "repository_history_records_keyset WHERE is_tombstone = 0 AND ",
                "observed_start < ?1 AND (observed_start, source_node_id, source_epoch, ",
                "stream, sequence) > (?2, ?3, ?4, ?5, ?6) ORDER BY observed_start, ",
                "source_node_id, source_epoch, stream, sequence LIMIT ?7"
            ),
            Self::RetentionExpiredRecordProbe => concat!(
                "SELECT 1 FROM repository_history_records INDEXED BY ",
                "repository_history_records_export_filter WHERE is_tombstone = 0 ",
                "AND observed_end < ?1 LIMIT 1"
            ),
            Self::RetentionReplaceAndPrune => concat!(
                "transaction: replace repository history rows; DELETE expired records; ",
                "DELETE expired segments; UPDATE history_snapshots; SQLite maintenance"
            ),
        }
    }

    fn is_composite(self) -> bool {
        matches!(
            self,
            Self::RuntimeStatus
                | Self::SourceDeliveryJournalSummary
                | Self::TieredBackfillPage
                | Self::TieredBackfillExportRefresh
                | Self::TieredBackfillExportWatermarks
                | Self::RetentionPrune
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct HistoryStorageDiagnosticEvent {
    operation_id: String,
    caller_class: String,
    statement: String,
    #[serde(skip)]
    composite: bool,
    started_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    finished_at_unix_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    elapsed_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct HistoryStorageDiagnosticState {
    #[serde(default = "diagnostic_schema_version")]
    schema_version: u32,
    process_start_unix_ms: u64,
    #[serde(default)]
    process_nonce: String,
    updated_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    in_flight: Option<HistoryStorageDiagnosticEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_slow_event: Option<HistoryStorageDiagnosticEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_slow_leaf_event: Option<HistoryStorageDiagnosticEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_interrupted_event: Option<HistoryStorageDiagnosticEvent>,
    #[serde(default)]
    completed_operations: u64,
    #[serde(default)]
    slow_operations: u64,
}

fn diagnostic_schema_version() -> u32 {
    DIAGNOSTIC_SCHEMA_VERSION
}

impl Default for HistoryStorageDiagnosticState {
    fn default() -> Self {
        Self {
            schema_version: DIAGNOSTIC_SCHEMA_VERSION,
            process_start_unix_ms: 0,
            process_nonce: String::new(),
            updated_at_unix_ms: 0,
            in_flight: None,
            last_slow_event: None,
            last_slow_leaf_event: None,
            last_interrupted_event: None,
            completed_operations: 0,
            slow_operations: 0,
        }
    }
}

pub(crate) struct HistoryStorageDiagnostics {
    path: PathBuf,
    state: Mutex<HistoryStorageDiagnosticState>,
    active_operations: Mutex<BTreeMap<u64, HistoryStorageDiagnosticEvent>>,
    next_scope_id: AtomicU64,
    persist_tx: SyncSender<()>,
    persist_writer_failed: AtomicBool,
    persist_failures: AtomicU64,
    persist_retry_after_unix_ms: AtomicU64,
}

impl HistoryStorageDiagnostics {
    fn open(data_dir: &Path) -> Arc<Self> {
        let process_start_unix_ms = unix_milliseconds(SystemTime::now());
        let process_nonce = Uuid::new_v4().to_string();
        let path = data_dir.join(HISTORY_STORAGE_DIAGNOSTICS_FILE);
        let (mut state, preserve_existing_file) = load_diagnostic_state(&path);
        if state.in_flight.is_some()
            && (state.process_nonce.is_empty() || state.process_nonce != process_nonce)
        {
            state.last_interrupted_event = state.in_flight.take();
        }
        state.schema_version = DIAGNOSTIC_SCHEMA_VERSION;
        state.process_start_unix_ms = process_start_unix_ms;
        state.process_nonce = process_nonce;
        state.updated_at_unix_ms = process_start_unix_ms;
        let (persist_tx, persist_rx) = mpsc::sync_channel(1);
        let diagnostics = Arc::new(Self {
            path,
            state: Mutex::new(state),
            active_operations: Mutex::new(BTreeMap::new()),
            next_scope_id: AtomicU64::new(1),
            persist_tx,
            persist_writer_failed: AtomicBool::new(false),
            persist_failures: AtomicU64::new(0),
            persist_retry_after_unix_ms: AtomicU64::new(0),
        });
        let weak = Arc::downgrade(&diagnostics);
        if let Err(error) = thread::Builder::new()
            .name("xp-history-diagnostics".to_owned())
            .spawn(move || persist_worker(weak, persist_rx))
        {
            diagnostics
                .persist_writer_failed
                .store(true, Ordering::Relaxed);
            warn!(error = %error, "failed to start history diagnostics writer");
        }
        if preserve_existing_file {
            diagnostics.notify_persist();
        }
        diagnostics
    }

    pub(crate) fn begin(
        self: &Arc<Self>,
        operation: HistoryStorageDiagnosticOperation,
        caller_class: &'static str,
    ) -> HistoryStorageDiagnosticGuard {
        let now = SystemTime::now();
        let started_at_unix_ms = unix_milliseconds(now);
        let event = HistoryStorageDiagnosticEvent {
            operation_id: operation.operation_id().to_owned(),
            caller_class: caller_class.to_owned(),
            statement: operation.statement().to_owned(),
            composite: operation.is_composite(),
            started_at_unix_ms,
            finished_at_unix_ms: None,
            elapsed_ms: None,
            outcome: None,
            result_count: None,
        };
        let scope_id = self.next_scope_id.fetch_add(1, Ordering::Relaxed);
        {
            let mut active_operations = self
                .active_operations
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            active_operations.insert(scope_id, event.clone());
            let mut state = self.lock_state();
            state.in_flight = active_operations
                .last_key_value()
                .map(|(_, event)| event.clone());
            state.updated_at_unix_ms = started_at_unix_ms;
        }
        self.notify_persist();
        HistoryStorageDiagnosticGuard {
            diagnostics: Arc::clone(self),
            scope_id,
            event,
            started_at: Instant::now(),
            finished: false,
        }
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, HistoryStorageDiagnosticState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn finish(
        &self,
        scope_id: u64,
        event: &mut HistoryStorageDiagnosticEvent,
        elapsed: Duration,
        outcome: &'static str,
    ) {
        let elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
        let finished_at_unix_ms = unix_milliseconds(SystemTime::now());
        event.finished_at_unix_ms = Some(finished_at_unix_ms);
        event.elapsed_ms = Some(elapsed_ms);
        event.outcome = Some(outcome.to_owned());
        {
            let mut active_operations = self
                .active_operations
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut state = self.lock_state();
            state.completed_operations = state.completed_operations.saturating_add(1);
            if elapsed_ms >= SLOW_OPERATION_MILLISECONDS {
                state.slow_operations = state.slow_operations.saturating_add(1);
                state.last_slow_event = Some(event.clone());
                if !event.composite {
                    state.last_slow_leaf_event = Some(event.clone());
                }
            }
            active_operations.remove(&scope_id);
            state.in_flight = active_operations
                .last_key_value()
                .map(|(_, event)| event.clone());
            state.updated_at_unix_ms = finished_at_unix_ms;
        }
        self.notify_persist();
    }

    fn notify_persist(&self) {
        match self.persist_tx.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => {}
            Err(TrySendError::Disconnected(())) => {
                if !self.persist_writer_failed.swap(true, Ordering::Relaxed) {
                    warn!(
                        path = %self.path.display(),
                        "history diagnostics writer is unavailable"
                    );
                }
            }
        }
    }

    fn persist_snapshot(&self) {
        let state = self.lock_state().clone();
        let Ok(mut bytes) = serde_json::to_vec_pretty(&state) else {
            return;
        };
        if bytes.len().saturating_add(1) > DIAGNOSTIC_MAX_BYTES {
            return;
        }
        bytes.push(b'\n');
        let now = unix_milliseconds(SystemTime::now());
        if now < self.persist_retry_after_unix_ms.load(Ordering::Relaxed) {
            return;
        }
        match write_atomic_private(&self.path, &bytes) {
            Ok(()) => {
                self.persist_failures.store(0, Ordering::Relaxed);
                self.persist_retry_after_unix_ms.store(0, Ordering::Relaxed);
            }
            Err(error) => {
                let failures = self
                    .persist_failures
                    .fetch_add(1, Ordering::Relaxed)
                    .saturating_add(1);
                let shift = failures.saturating_sub(1).min(6);
                let delay = DIAGNOSTIC_RETRY_BASE_MILLISECONDS
                    .saturating_mul(1_u64 << shift)
                    .min(DIAGNOSTIC_RETRY_MAX_MILLISECONDS);
                self.persist_retry_after_unix_ms
                    .store(now.saturating_add(delay), Ordering::Relaxed);
                warn!(
                    path = %self.path.display(),
                    error = %error,
                    retry_after_ms = delay,
                    "history diagnostics persistence failed"
                );
            }
        }
    }
}

fn load_diagnostic_state(path: &Path) -> (HistoryStorageDiagnosticState, bool) {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return (HistoryStorageDiagnosticState::default(), true);
        }
        Err(error) => {
            warn!(
                path = %path.display(),
                error = %error,
                "history diagnostics file could not be read"
            );
            return (HistoryStorageDiagnosticState::default(), false);
        }
    };
    if file
        .metadata()
        .map(|metadata| metadata.len() > u64::try_from(DIAGNOSTIC_MAX_BYTES).unwrap_or(u64::MAX))
        .unwrap_or(true)
    {
        warn!(path = %path.display(), "history diagnostics file exceeds the size limit");
        return (HistoryStorageDiagnosticState::default(), false);
    }
    let mut bytes = Vec::with_capacity(DIAGNOSTIC_MAX_BYTES);
    if file
        .take(
            u64::try_from(DIAGNOSTIC_MAX_BYTES)
                .unwrap_or(u64::MAX)
                .saturating_add(1),
        )
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() > DIAGNOSTIC_MAX_BYTES
    {
        warn!(path = %path.display(), "history diagnostics file could not be bounded-read");
        return (HistoryStorageDiagnosticState::default(), false);
    }
    match serde_json::from_slice(&bytes) {
        Ok(state) => (state, true),
        Err(error) => {
            warn!(path = %path.display(), error = %error, "history diagnostics file is invalid");
            (HistoryStorageDiagnosticState::default(), false)
        }
    }
}

fn persist_worker(weak: Weak<HistoryStorageDiagnostics>, receiver: Receiver<()>) {
    let mut last_persist = Instant::now()
        .checked_sub(DIAGNOSTIC_PERSIST_INTERVAL)
        .unwrap_or_else(Instant::now);
    loop {
        let retry_after = weak.upgrade().map(|diagnostics| {
            diagnostics
                .persist_retry_after_unix_ms
                .load(Ordering::Relaxed)
        });
        let now = unix_milliseconds(SystemTime::now());
        let received = if let Some(retry_after) = retry_after.filter(|deadline| *deadline > now) {
            receiver.recv_timeout(Duration::from_millis(retry_after.saturating_sub(now)))
        } else {
            receiver.recv().map_err(|_| RecvTimeoutError::Disconnected)
        };
        match received {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        while receiver.try_recv().is_ok() {}
        let elapsed = last_persist.elapsed();
        if elapsed < DIAGNOSTIC_PERSIST_INTERVAL {
            match receiver.recv_timeout(DIAGNOSTIC_PERSIST_INTERVAL - elapsed) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            while receiver.try_recv().is_ok() {}
        }
        let Some(diagnostics) = weak.upgrade() else {
            return;
        };
        diagnostics.persist_snapshot();
        last_persist = Instant::now();
    }
}

pub(crate) struct HistoryStorageDiagnosticGuard {
    diagnostics: Arc<HistoryStorageDiagnostics>,
    scope_id: u64,
    event: HistoryStorageDiagnosticEvent,
    started_at: Instant,
    finished: bool,
}

impl HistoryStorage {
    pub(crate) fn begin_diagnostic(
        &self,
        operation: HistoryStorageDiagnosticOperation,
        caller_class: &'static str,
    ) -> HistoryStorageDiagnosticGuard {
        self.diagnostics.begin(operation, caller_class)
    }
}

impl HistoryStorageDiagnosticGuard {
    pub(crate) fn finish(mut self) {
        self.finish_with_outcome("completed", None);
    }

    pub(crate) fn finish_with_count(mut self, result_count: usize) {
        self.finish_with_outcome("completed", Some(result_count));
    }

    fn finish_with_outcome(&mut self, outcome: &'static str, result_count: Option<usize>) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.event.result_count = result_count;
        self.diagnostics.finish(
            self.scope_id,
            &mut self.event,
            self.started_at.elapsed(),
            outcome,
        );
    }
}

impl Drop for HistoryStorageDiagnosticGuard {
    fn drop(&mut self) {
        self.finish_with_outcome("scope_exit", None);
    }
}

pub(crate) fn shared_history_storage_diagnostics(
    data_dir: &Path,
) -> Arc<HistoryStorageDiagnostics> {
    let registry = diagnostics_registry();
    let mut registry = registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    registry.retain(|_, diagnostics| diagnostics.strong_count() > 0);
    if let Some(diagnostics) = registry.get(data_dir).and_then(Weak::upgrade) {
        return diagnostics;
    }
    let diagnostics = HistoryStorageDiagnostics::open(data_dir);
    registry.insert(data_dir.to_path_buf(), Arc::downgrade(&diagnostics));
    diagnostics
}

fn diagnostics_registry() -> &'static Mutex<BTreeMap<PathBuf, Weak<HistoryStorageDiagnostics>>> {
    static REGISTRY: OnceLock<Mutex<BTreeMap<PathBuf, Weak<HistoryStorageDiagnostics>>>> =
        OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn unix_milliseconds(now: SystemTime) -> u64 {
    now.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn persists_in_flight_and_slow_operation_without_bind_values() {
        let temporary = tempdir().unwrap();
        let diagnostics = HistoryStorageDiagnostics::open(temporary.path());
        let guard = diagnostics.begin(
            HistoryStorageDiagnosticOperation::RuntimeStatusRecordCount,
            "test.runtime_status",
        );
        let path = temporary.path().join(HISTORY_STORAGE_DIAGNOSTICS_FILE);
        wait_for_state(&path, |state| state.in_flight.is_some());
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("runtime_status.record_count"));
        assert!(raw.contains("repository_history_records_keyset"));
        assert!(!raw.contains("bind"));

        std::thread::sleep(Duration::from_millis(2));
        guard.finish_with_count(12);
        let state = wait_for_state(&path, |state| {
            state.in_flight.is_none() && state.last_slow_event.is_some()
        });
        assert!(state.in_flight.is_none());
        assert_eq!(state.last_slow_event.unwrap().result_count, Some(12));
        assert!(state.completed_operations >= 1);
    }

    #[test]
    fn recovers_an_interrupted_operation_on_restart() {
        let temporary = tempdir().unwrap();
        let diagnostics = HistoryStorageDiagnostics::open(temporary.path());
        let guard = diagnostics.begin(
            HistoryStorageDiagnosticOperation::TieredBackfillReceivedAtCutoff,
            "test.backfill",
        );
        std::mem::forget(guard);
        wait_for_state(
            &temporary.path().join(HISTORY_STORAGE_DIAGNOSTICS_FILE),
            |state| state.in_flight.is_some(),
        );
        drop(diagnostics);

        let restarted = HistoryStorageDiagnostics::open(temporary.path());
        let state = restarted.lock_state().clone();
        assert!(state.in_flight.is_none());
        assert_eq!(
            state.last_interrupted_event.unwrap().operation_id,
            "tiered_backfill.export_watermarks.received_at_cutoff"
        );
    }

    #[test]
    fn overlapping_operations_do_not_restore_a_completed_scope() {
        let temporary = tempdir().unwrap();
        let diagnostics = HistoryStorageDiagnostics::open(temporary.path());
        let first = diagnostics.begin(
            HistoryStorageDiagnosticOperation::RuntimeStatus,
            "test.first",
        );
        let second = diagnostics.begin(
            HistoryStorageDiagnosticOperation::RuntimeStatusRecordCount,
            "test.second",
        );

        first.finish();
        let state = diagnostics.lock_state().clone();
        assert_eq!(
            state.in_flight.unwrap().operation_id,
            "runtime_status.record_count"
        );

        second.finish();
        assert!(diagnostics.lock_state().in_flight.is_none());
    }

    #[test]
    fn diagnostic_state_stays_bounded() {
        let temporary = tempdir().unwrap();
        let diagnostics = HistoryStorageDiagnostics::open(temporary.path());
        let guard = diagnostics.begin(
            HistoryStorageDiagnosticOperation::RetentionReplaceAndPrune,
            "test.retention",
        );
        guard.finish();
        let path = temporary.path().join(HISTORY_STORAGE_DIAGNOSTICS_FILE);
        wait_for_state(&path, |state| state.in_flight.is_none());
        let bytes = fs::read(path).unwrap();
        assert!(bytes.len() <= DIAGNOSTIC_MAX_BYTES);
    }

    #[cfg(unix)]
    #[test]
    fn diagnostic_file_is_private() {
        use std::os::unix::fs::PermissionsExt;

        let temporary = tempdir().unwrap();
        let diagnostics = HistoryStorageDiagnostics::open(temporary.path());
        let path = temporary.path().join(HISTORY_STORAGE_DIAGNOSTICS_FILE);
        wait_for_state(&path, |_| true);
        let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        drop(diagnostics);
    }

    #[test]
    fn nested_slow_operation_preserves_leaf_event() {
        let temporary = tempdir().unwrap();
        let diagnostics = HistoryStorageDiagnostics::open(temporary.path());
        let outer = diagnostics.begin(
            HistoryStorageDiagnosticOperation::RuntimeStatus,
            "test.outer",
        );
        let inner = diagnostics.begin(
            HistoryStorageDiagnosticOperation::RuntimeStatusRecordCount,
            "test.inner",
        );
        inner.finish();
        outer.finish();

        let state = diagnostics.lock_state().clone();
        assert_eq!(
            state.last_slow_leaf_event.unwrap().operation_id,
            "runtime_status.record_count"
        );
        assert_eq!(
            state.last_slow_event.unwrap().operation_id,
            "runtime_status"
        );
        let path = temporary.path().join(HISTORY_STORAGE_DIAGNOSTICS_FILE);
        let persisted = wait_for_state(&path, |state| state.last_slow_leaf_event.is_some());
        assert_eq!(
            persisted.last_slow_leaf_event.unwrap().operation_id,
            "runtime_status.record_count"
        );
    }

    fn wait_for_state(
        path: &Path,
        predicate: impl Fn(&HistoryStorageDiagnosticState) -> bool,
    ) -> HistoryStorageDiagnosticState {
        for _ in 0..100 {
            if let Ok(bytes) = fs::read(path) {
                if let Ok(state) = serde_json::from_slice::<HistoryStorageDiagnosticState>(&bytes) {
                    if predicate(&state) {
                        return state;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out waiting for history diagnostics persistence");
    }
}
