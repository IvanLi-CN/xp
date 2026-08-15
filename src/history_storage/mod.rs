//! Local SQLite persistence for bounded ordinary-node history snapshots.
//!
//! Callers retain ownership of their in-memory models and JSON serialization. This boundary only
//! changes where those snapshots are durably stored, so sampling and retention semantics remain
//! local to their existing modules.

use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension, params};
use tracing::warn;

pub(crate) const STATE_KEY: &str = "persistent_state";
pub(crate) const USAGE_KEY: &str = "traffic_usage";
pub(crate) const INBOUND_IP_USAGE_KEY: &str = "inbound_ip_usage";
pub(crate) const TCP_CONNECTION_USAGE_KEY: &str = "tcp_connection_usage";
pub(crate) const NODE_HISTORY_KEY: &str = "node_history";
pub(crate) const MESH_TELEMETRY_KEY: &str = "mesh_telemetry";
pub(crate) const REPOSITORY_REPLICA_KEY: &str = "repository_replica";

mod repository;
#[allow(unused_imports)]
pub(crate) use repository::{
    RepositoryHistoryCompactionCursor, RepositoryHistoryCoverage, RepositoryHistoryRecordRow,
    RepositoryHistorySegmentRow, RepositoryHistoryTombstone, RepositoryReplicaMutation,
};

const SQLITE_FILE: &str = "history.sqlite3";
const SQLITE_STAGING_FILE: &str = "history.sqlite3.migrating";
const JSON_FALLBACK_FILE: &str = "history.sqlite3.json-fallback";
const BACKUP_RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const CHECKPOINT_PAGES: u32 = 64;
const VACUUM_PAGES: u32 = 64;

const SOURCES: [HistorySource; 7] = [
    HistorySource::new(STATE_KEY, "state.json"),
    HistorySource::new(USAGE_KEY, "usage.json"),
    HistorySource::new(INBOUND_IP_USAGE_KEY, "inbound_ip_usage.json"),
    HistorySource::new(TCP_CONNECTION_USAGE_KEY, "tcp_connection_usage.json"),
    HistorySource::new(NODE_HISTORY_KEY, "node_history_cache.json"),
    HistorySource::new(MESH_TELEMETRY_KEY, "mesh/telemetry.json"),
    HistorySource::new(REPOSITORY_REPLICA_KEY, "history/repository_replica.json"),
];

#[derive(Clone, Copy)]
struct HistorySource {
    key: &'static str,
    relative_path: &'static str,
}

impl HistorySource {
    const fn new(key: &'static str, relative_path: &'static str) -> Self {
        Self { key, relative_path }
    }

    fn path(self, data_dir: &Path) -> PathBuf {
        data_dir.join(self.relative_path)
    }
}

#[derive(Debug)]
pub(crate) struct HistoryStorageError(String);

const REPOSITORY_HISTORY_EXPORT_LEASE_SECONDS: u64 = 15 * 60;
const MAX_ACTIVE_REPOSITORY_HISTORY_EXPORTS: usize = 4;

impl std::fmt::Display for HistoryStorageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for HistoryStorageError {}

type Result<T> = std::result::Result<T, HistoryStorageError>;

#[derive(Clone)]
pub(crate) struct HistoryStorage {
    data_dir: Arc<PathBuf>,
    backend: Arc<Mutex<Backend>>,
    #[cfg(test)]
    fail_maintenance_after_commit: Arc<std::sync::atomic::AtomicBool>,
}

impl std::fmt::Debug for HistoryStorage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HistoryStorage")
            .field("data_dir", &self.data_dir)
            .field("sqlite", &self.is_sqlite())
            .finish()
    }
}

pub(crate) enum Backend {
    Sqlite(Connection),
    Json,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HistoryStorageMode {
    Sqlite,
    DegradedJson,
}

impl HistoryStorage {
    pub(crate) fn open(data_dir: &Path) -> Self {
        let data_dir = Arc::new(normalize_data_dir(data_dir));
        let backend = shared_backend(&data_dir);
        let storage = Self {
            data_dir,
            backend,
            #[cfg(test)]
            fail_maintenance_after_commit: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        storage.cleanup_expired_backups_at(SystemTime::now());
        storage
    }

    pub(crate) fn read(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let mut backend = self.lock_backend();
        match &mut *backend {
            Backend::Sqlite(connection) => match read_sqlite(connection, key) {
                Ok(value) => Ok(value),
                Err(error) => {
                    warn!(
                        error = %error,
                        key,
                        history_storage_mode = "degraded_json",
                        "history storage degraded; returning to JSON snapshots"
                    );
                    switch_to_json(&mut backend, &self.data_dir);
                    if matches!(*backend, Backend::Json) {
                        read_json(source_path(&self.data_dir, key))
                    } else {
                        Err(error)
                    }
                }
            },
            Backend::Json => read_json(source_path(&self.data_dir, key)),
        }
    }

    pub(crate) fn write(&self, key: &str, payload: &[u8]) -> Result<()> {
        let mut backend = self.lock_backend();
        match &mut *backend {
            Backend::Sqlite(connection) => match write_sqlite(connection, key, payload) {
                Ok(()) => Ok(()),
                Err(error) => {
                    warn!(
                        error = %error,
                        key,
                        history_storage_mode = "degraded_json",
                        "history storage degraded; returning to JSON snapshots"
                    );
                    switch_to_json(&mut backend, &self.data_dir);
                    if matches!(*backend, Backend::Json) {
                        write_json(source_path(&self.data_dir, key), payload)
                    } else {
                        Err(error)
                    }
                }
            },
            Backend::Json => write_json(source_path(&self.data_dir, key), payload),
        }
    }

    pub(crate) fn is_sqlite(&self) -> bool {
        self.mode() == HistoryStorageMode::Sqlite
    }

    pub(crate) fn mode(&self) -> HistoryStorageMode {
        match &*self.lock_backend() {
            Backend::Sqlite(_) => HistoryStorageMode::Sqlite,
            Backend::Json => HistoryStorageMode::DegradedJson,
        }
    }

    pub(crate) fn degrade_to_json(&self) -> bool {
        let mut backend = self.lock_backend();
        switch_to_json(&mut backend, &self.data_dir);
        matches!(*backend, Backend::Json)
    }

    pub(crate) fn available_bytes(&self) -> io::Result<u64> {
        let c_path =
            std::ffi::CString::new(self.data_dir.as_os_str().as_encoded_bytes()).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "history path contains NUL")
            })?;
        let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        let result = unsafe { libc::statvfs(c_path.as_ptr(), stats.as_mut_ptr()) };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        let stats = unsafe { stats.assume_init() };
        #[cfg(target_os = "macos")]
        let available_blocks = u64::from(stats.f_bavail);
        #[cfg(not(target_os = "macos"))]
        let available_blocks = stats.f_bavail;
        Ok(available_blocks.saturating_mul(stats.f_frsize))
    }

    /// Allocate a fresh source epoch outside the replica control blob. When that blob is rebuilt
    /// while SQLite remains intact, a new source chain cannot reuse sequence zero under its old
    /// epoch.
    fn cleanup_expired_backups_at(&self, now: SystemTime) {
        let mut backend = self.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            return;
        };
        let Ok(Some(completed_at)) = read_meta_i64(connection, "migration_completed_at") else {
            return;
        };
        let completed_at = UNIX_EPOCH + Duration::from_secs(completed_at as u64);
        if now.duration_since(completed_at).unwrap_or_default() < BACKUP_RETENTION {
            return;
        }

        for source in SOURCES {
            let meta_key = migrated_meta_key(source.key);
            if read_meta_i64(connection, &meta_key)
                .ok()
                .flatten()
                .is_none()
            {
                continue;
            }
            let path = source.path(&self.data_dir);
            if let Err(error) = fs::remove_file(&path)
                && error.kind() != io::ErrorKind::NotFound
            {
                warn!(error = %error, path = %path.display(), "remove expired JSON history backup");
            }
        }
    }

    pub(crate) fn lock_backend(&self) -> std::sync::MutexGuard<'_, Backend> {
        self.backend
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[cfg(test)]
    pub(crate) fn set_query_only_for_test(&self, enabled: bool) -> Result<()> {
        let mut backend = self.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            return Err(HistoryStorageError(
                "query-only test hook requires SQLite".to_owned(),
            ));
        };
        connection
            .pragma_update(None, "query_only", if enabled { "ON" } else { "OFF" })
            .map_err(sqlite_error)
    }

    #[cfg(test)]
    pub(crate) fn set_maintenance_failure_for_test(&self, enabled: bool) {
        self.fail_maintenance_after_commit
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }
}

fn shared_backend(data_dir: &Path) -> Arc<Mutex<Backend>> {
    let registry = backend_registry();
    let mut registry = registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    registry.retain(|_, backend| backend.strong_count() > 0);
    if let Some(backend) = registry.get(data_dir).and_then(Weak::upgrade) {
        return backend;
    }

    let backend = if json_fallback_path(data_dir).exists() {
        Backend::Json
    } else {
        match open_sqlite(data_dir) {
            Ok(connection) => Backend::Sqlite(connection),
            Err(error) => {
                warn!(
                    error = %error,
                    path = %data_dir.join(SQLITE_FILE).display(),
                    history_storage_mode = "degraded_json",
                    "history storage degraded; continuing with JSON snapshots"
                );
                Backend::Json
            }
        }
    };
    let backend = Arc::new(Mutex::new(backend));
    registry.insert(data_dir.to_path_buf(), Arc::downgrade(&backend));
    backend
}

fn backend_registry() -> &'static Mutex<BTreeMap<PathBuf, Weak<Mutex<Backend>>>> {
    static REGISTRY: OnceLock<Mutex<BTreeMap<PathBuf, Weak<Mutex<Backend>>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn normalize_data_dir(data_dir: &Path) -> PathBuf {
    fs::canonicalize(data_dir).unwrap_or_else(|_| {
        if data_dir.is_absolute() {
            data_dir.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|current_dir| current_dir.join(data_dir))
                .unwrap_or_else(|_| data_dir.to_path_buf())
        }
    })
}

fn json_fallback_path(data_dir: &Path) -> PathBuf {
    data_dir.join(JSON_FALLBACK_FILE)
}

fn open_sqlite(data_dir: &Path) -> Result<Connection> {
    fs::create_dir_all(data_dir).map_err(io_error)?;
    let db_path = data_dir.join(SQLITE_FILE);
    if db_path.exists() {
        let connection = Connection::open(&db_path).map_err(sqlite_error)?;
        configure_runtime(&connection)?;
        ensure_schema(&connection)?;
        return Ok(connection);
    }

    migrate_json_snapshots(data_dir, &db_path)?;
    let connection = Connection::open(db_path).map_err(sqlite_error)?;
    configure_runtime(&connection)?;
    ensure_schema(&connection)?;
    Ok(connection)
}

fn migrate_json_snapshots(data_dir: &Path, db_path: &Path) -> Result<()> {
    let staging_path = data_dir.join(SQLITE_STAGING_FILE);
    if staging_path.exists() {
        fs::remove_file(&staging_path).map_err(io_error)?;
    }

    let result = (|| {
        let mut connection = Connection::open(&staging_path).map_err(sqlite_error)?;
        // SQLite only applies this setting to a newly created database before schema objects exist.
        connection
            .pragma_update(None, "auto_vacuum", "INCREMENTAL")
            .map_err(sqlite_error)?;
        ensure_schema(&connection)?;

        let transaction = connection.transaction().map_err(sqlite_error)?;
        for source in SOURCES {
            let path = source.path(data_dir);
            if !path.exists() {
                continue;
            }
            let payload = fs::read(&path).map_err(io_error)?;
            transaction
                .execute(
                    "INSERT INTO history_snapshots (key, payload, updated_at) VALUES (?1, ?2, ?3)",
                    params![source.key, payload, unix_seconds(SystemTime::now())],
                )
                .map_err(sqlite_error)?;
            write_meta_i64(&transaction, &migrated_meta_key(source.key), 1)?;
        }
        write_meta_i64(
            &transaction,
            "migration_completed_at",
            unix_seconds(SystemTime::now()),
        )?;
        transaction.commit().map_err(sqlite_error)?;

        // The staged migration uses the rollback journal so one rename publishes all records.
        // Runtime opens the published database in WAL mode below.
        drop(connection);
        fs::rename(&staging_path, db_path).map_err(io_error)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&staging_path);
    }
    result
}

fn configure_runtime(connection: &Connection) -> Result<()> {
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(sqlite_error)?;
    connection
        .pragma_update(None, "wal_autocheckpoint", CHECKPOINT_PAGES)
        .map_err(sqlite_error)?;
    Ok(())
}

fn ensure_schema(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS history_snapshots (
                key TEXT PRIMARY KEY NOT NULL,
                payload BLOB NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS history_storage_meta (
                key TEXT PRIMARY KEY NOT NULL,
                value INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS repository_history_records (
                source_node_id TEXT NOT NULL,
                source_epoch INTEGER NOT NULL,
                stream TEXT NOT NULL,
                sequence INTEGER NOT NULL,
                subject_node_id TEXT NOT NULL,
                observer_node_id TEXT NOT NULL DEFAULT '',
                schema_id TEXT NOT NULL DEFAULT '',
                schema_version INTEGER NOT NULL DEFAULT 0,
                record_key BLOB NOT NULL DEFAULT X'',
                is_tombstone INTEGER NOT NULL DEFAULT 0,
                observed_start INTEGER NOT NULL,
                observed_end INTEGER NOT NULL,
                received_at INTEGER NOT NULL,
                aggregate_complete INTEGER,
                aggregate_start INTEGER,
                aggregate_end INTEGER,
                payload BLOB NOT NULL,
                PRIMARY KEY (source_node_id, source_epoch, stream, sequence)
            );
            CREATE INDEX IF NOT EXISTS repository_history_records_query
                ON repository_history_records
                    (subject_node_id, observed_start, observed_end, source_node_id,
                     source_epoch, stream, sequence);
            CREATE TABLE IF NOT EXISTS repository_history_segments (
                id TEXT PRIMARY KEY NOT NULL,
                closed_at INTEGER NOT NULL DEFAULT 0,
                payload BLOB NOT NULL
            );
            CREATE TABLE IF NOT EXISTS repository_history_export_leases (
                session_id TEXT PRIMARY KEY NOT NULL,
                expires_at INTEGER NOT NULL
            );
            ",
        )
        .map_err(sqlite_error)?;
    ensure_repository_history_columns(connection)?;
    ensure_repository_history_segment_columns(connection)?;
    connection
        .execute_batch(
            "CREATE INDEX IF NOT EXISTS repository_history_records_tombstone
              ON repository_history_records
                 (source_node_id, source_epoch, stream, subject_node_id, observer_node_id,
                  schema_id, schema_version, record_key, is_tombstone);
             CREATE INDEX IF NOT EXISTS repository_history_records_aggregate_completeness
               ON repository_history_records
                  (subject_node_id, aggregate_complete, aggregate_start, aggregate_end);
             CREATE INDEX IF NOT EXISTS repository_history_records_keyset
               ON repository_history_records
                  (is_tombstone, observed_start, source_node_id, source_epoch, stream, sequence,
                   observed_end, received_at);
             CREATE INDEX IF NOT EXISTS repository_history_records_export_filter
               ON repository_history_records
                  (is_tombstone, observed_end, received_at, observed_start, source_node_id,
                   source_epoch, stream, sequence);",
        )
        .map_err(sqlite_error)
}

fn ensure_repository_history_segment_columns(connection: &Connection) -> Result<()> {
    let mut statement = connection
        .prepare("PRAGMA table_info(repository_history_segments)")
        .map_err(sqlite_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(sqlite_error)?
        .collect::<std::result::Result<std::collections::BTreeSet<_>, _>>()
        .map_err(sqlite_error)?;
    if !columns.contains("closed_at") {
        connection
            .execute(
                "ALTER TABLE repository_history_segments
                 ADD COLUMN closed_at INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .map_err(sqlite_error)?;
    }
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS repository_history_segments_closed_at
             ON repository_history_segments (closed_at ASC, id ASC)",
            [],
        )
        .map(|_| ())
        .map_err(sqlite_error)
}

fn ensure_repository_history_columns(connection: &Connection) -> Result<()> {
    let mut statement = connection
        .prepare("PRAGMA table_info(repository_history_records)")
        .map_err(sqlite_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(sqlite_error)?
        .collect::<std::result::Result<std::collections::BTreeSet<_>, _>>()
        .map_err(sqlite_error)?;
    for (name, definition) in [
        ("observer_node_id", "TEXT NOT NULL DEFAULT ''"),
        ("schema_id", "TEXT NOT NULL DEFAULT ''"),
        ("schema_version", "INTEGER NOT NULL DEFAULT 0"),
        ("record_key", "BLOB NOT NULL DEFAULT X''"),
        ("is_tombstone", "INTEGER NOT NULL DEFAULT 0"),
        ("aggregate_complete", "INTEGER"),
        ("aggregate_start", "INTEGER"),
        ("aggregate_end", "INTEGER"),
    ] {
        if !columns.contains(name) {
            connection
                .execute(
                    &format!(
                        "ALTER TABLE repository_history_records ADD COLUMN {name} {definition}"
                    ),
                    [],
                )
                .map_err(sqlite_error)?;
        }
    }
    Ok(())
}

fn repository_history_record_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<RepositoryHistoryRecordRow> {
    Ok(RepositoryHistoryRecordRow {
        source_node_id: row.get(0)?,
        source_epoch: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(u64::MAX),
        stream: row.get(2)?,
        sequence: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(u64::MAX),
        subject_node_id: row.get(4)?,
        observer_node_id: row.get(5)?,
        schema_id: row.get(6)?,
        schema_version: u32::try_from(row.get::<_, i64>(7)?).unwrap_or(u32::MAX),
        record_key: row.get(8)?,
        tombstone: row.get(9)?,
        observed_start_unix_seconds: u64::try_from(row.get::<_, i64>(10)?).unwrap_or(u64::MAX),
        observed_end_unix_seconds: u64::try_from(row.get::<_, i64>(11)?).unwrap_or(u64::MAX),
        received_at_unix_seconds: u64::try_from(row.get::<_, i64>(12)?).unwrap_or(u64::MAX),
        aggregate_complete: None,
        aggregate_start_unix_seconds: None,
        aggregate_end_unix_seconds: None,
        payload: row.get(13)?,
    })
}

fn read_sqlite(connection: &Connection, key: &str) -> Result<Option<Vec<u8>>> {
    connection
        .query_row(
            "SELECT payload FROM history_snapshots WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_error)
}

fn write_sqlite(connection: &mut Connection, key: &str, payload: &[u8]) -> Result<()> {
    let transaction = connection.transaction().map_err(sqlite_error)?;
    write_snapshot(&transaction, key, payload)?;
    transaction.commit().map_err(sqlite_error)?;

    maintain_sqlite(connection)
}

fn write_snapshot(
    transaction: &rusqlite::Transaction<'_>,
    key: &str,
    payload: &[u8],
) -> Result<()> {
    transaction
        .execute(
            "
            INSERT INTO history_snapshots (key, payload, updated_at) VALUES (?1, ?2, ?3)
            ON CONFLICT(key) DO UPDATE SET
                payload = excluded.payload,
                updated_at = excluded.updated_at
            ",
            params![key, payload, unix_seconds(SystemTime::now())],
        )
        .map(|_| ())
        .map_err(sqlite_error)
}

fn upsert_repository_history_record(
    transaction: &rusqlite::Transaction<'_>,
    row: &RepositoryHistoryRecordRow,
) -> Result<()> {
    transaction
        .execute(
            "
            INSERT INTO repository_history_records (
                source_node_id, source_epoch, stream, sequence, subject_node_id,
                observer_node_id, schema_id, schema_version, record_key, is_tombstone,
                observed_start, observed_end, received_at, aggregate_complete,
                aggregate_start, aggregate_end, payload
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17
            )
            ON CONFLICT(source_node_id, source_epoch, stream, sequence) DO UPDATE SET
                subject_node_id = excluded.subject_node_id,
                observer_node_id = excluded.observer_node_id,
                schema_id = excluded.schema_id,
                schema_version = excluded.schema_version,
                record_key = excluded.record_key,
                is_tombstone = excluded.is_tombstone,
                observed_start = excluded.observed_start,
                observed_end = excluded.observed_end,
                received_at = excluded.received_at,
                aggregate_complete = excluded.aggregate_complete,
                aggregate_start = excluded.aggregate_start,
                aggregate_end = excluded.aggregate_end,
                payload = excluded.payload
            ",
            params![
                row.source_node_id,
                durable_i64(row.source_epoch, "source epoch")?,
                row.stream,
                durable_i64(row.sequence, "sequence")?,
                row.subject_node_id,
                row.observer_node_id,
                row.schema_id,
                i64::from(row.schema_version),
                row.record_key,
                row.tombstone,
                i64::try_from(row.observed_start_unix_seconds).unwrap_or(i64::MAX),
                i64::try_from(row.observed_end_unix_seconds).unwrap_or(i64::MAX),
                i64::try_from(row.received_at_unix_seconds).unwrap_or(i64::MAX),
                row.aggregate_complete,
                row.aggregate_start_unix_seconds
                    .map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
                row.aggregate_end_unix_seconds
                    .map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
                row.payload,
            ],
        )
        .map(|_| ())
        .map_err(sqlite_error)
}

fn upsert_repository_history_segment(
    transaction: &rusqlite::Transaction<'_>,
    row: &RepositoryHistorySegmentRow,
) -> Result<()> {
    transaction
        .execute(
            "INSERT INTO repository_history_segments (id, closed_at, payload)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET
               closed_at = excluded.closed_at,
               payload = excluded.payload",
            params![
                row.id,
                i64::try_from(row.closed_at_unix_seconds).unwrap_or(i64::MAX),
                row.payload,
            ],
        )
        .map(|_| ())
        .map_err(sqlite_error)
}

fn delete_repository_history_for_tombstone(
    connection: &rusqlite::Transaction<'_>,
    tombstone: &RepositoryHistoryTombstone,
) -> Result<usize> {
    let key_predicate = if tombstone.prefix {
        "substr(record_key, 1, length(?6)) = ?6"
    } else {
        "record_key = ?6"
    };
    let stream_predicate = if tombstone.prefix {
        "1 = 1"
    } else {
        "stream = ?1"
    };
    connection
        .execute(
            &format!(
                "DELETE FROM repository_history_records
                 WHERE {stream_predicate}
                   AND subject_node_id = ?2 AND observer_node_id = ?3
                   AND schema_id = ?4 AND schema_version = ?5 AND {key_predicate}
                   AND is_tombstone = 0"
            ),
            params![
                tombstone.stream,
                tombstone.subject_node_id,
                tombstone.observer_node_id,
                tombstone.schema_id,
                i64::from(tombstone.schema_version),
                tombstone.record_key,
            ],
        )
        .map_err(sqlite_error)
}

fn maintain_sqlite(connection: &Connection) -> Result<()> {
    // Both operations are page-bounded and avoid a blocking full checkpoint or VACUUM.
    connection
        .pragma_update(None, "wal_autocheckpoint", CHECKPOINT_PAGES)
        .map_err(sqlite_error)?;
    connection
        .execute_batch(&format!("PRAGMA incremental_vacuum({VACUUM_PAGES})"))
        .map_err(sqlite_error)?;
    Ok(())
}

fn finish_post_commit_maintenance(result: Result<()>) {
    if let Err(error) = result {
        warn!(
            error = %error,
            "repository mutation committed; deferred bounded SQLite maintenance"
        );
    }
}

fn switch_to_json(backend: &mut Backend, data_dir: &Path) {
    let Backend::Sqlite(connection) = backend else {
        return;
    };
    if repository_history_is_external(connection) {
        warn!(
            history_storage_mode = "sqlite_degraded",
            "keeping SQLite active because repository history cannot use JSON fallback"
        );
        return;
    }
    for source in SOURCES {
        match read_sqlite(connection, source.key) {
            Ok(Some(payload)) => {
                if let Err(error) = write_json(source.path(data_dir), &payload) {
                    warn!(
                        error = %error,
                        key = source.key,
                        "restore JSON snapshot after SQLite failure"
                    );
                }
            }
            Ok(None) => {}
            Err(error) => warn!(
                error = %error,
                key = source.key,
                "read SQLite snapshot while restoring JSON fallback"
            ),
        }
    }
    if let Err(error) = mark_json_fallback(data_dir) {
        warn!(error = %error, "record persistent JSON history fallback");
    }
    *backend = Backend::Json;
}

fn repository_history_is_external(connection: &Connection) -> bool {
    read_sqlite(connection, REPOSITORY_REPLICA_KEY)
        .ok()
        .flatten()
        .and_then(|payload| serde_json::from_slice::<serde_json::Value>(&payload).ok())
        .and_then(|snapshot| {
            snapshot
                .get("external_history")
                .and_then(serde_json::Value::as_bool)
        })
        .unwrap_or(false)
}

fn source_path(data_dir: &Path, key: &str) -> PathBuf {
    SOURCES
        .iter()
        .find(|source| source.key == key)
        .map(|source| source.path(data_dir))
        .unwrap_or_else(|| data_dir.join(format!("{key}.json")))
}

fn repository_source_epoch_meta_key(cluster_id: &str, node_id: &str) -> String {
    let mut hasher = sha2::Sha256::new();
    use sha2::Digest as _;
    hasher.update(b"xp-history-source-epoch-meta-v1\0");
    hasher.update(cluster_id.as_bytes());
    hasher.update([0]);
    hasher.update(node_id.as_bytes());
    format!("repository_source_epoch:{}", hex::encode(hasher.finalize()))
}

fn read_json(path: PathBuf) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(payload) => Ok(Some(payload)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(error)),
    }
}

fn write_json(path: PathBuf, payload: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    write_atomic_file(&path, payload)
}

fn mark_json_fallback(data_dir: &Path) -> Result<()> {
    write_atomic_file(&json_fallback_path(data_dir), b"json-fallback\n")
}

fn write_atomic_file(path: &Path, payload: &[u8]) -> Result<()> {
    let temporary = path.with_file_name(format!(
        "{}.tmp",
        path.file_name()
            .ok_or_else(|| io_error(io::Error::other("history snapshot has no filename")))?
            .to_string_lossy()
    ));
    {
        let mut file = fs::File::create(&temporary).map_err(io_error)?;
        file.write_all(payload).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
    }
    fs::rename(temporary, path).map_err(io_error)
}

fn read_meta_i64(connection: &Connection, key: &str) -> Result<Option<i64>> {
    connection
        .query_row(
            "SELECT value FROM history_storage_meta WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_error)
}

fn write_meta_i64(connection: &rusqlite::Transaction<'_>, key: &str, value: i64) -> Result<()> {
    connection
        .execute(
            "INSERT INTO history_storage_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
        .map_err(sqlite_error)?;
    Ok(())
}

fn durable_i64(value: u64, field: &str) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| HistoryStorageError(format!("repository {field} exceeds SQLite range")))
}

fn migrated_meta_key(key: &str) -> String {
    format!("migrated:{key}")
}

fn unix_seconds(now: SystemTime) -> i64 {
    now.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .try_into()
        .unwrap_or(i64::MAX)
}

fn io_error(error: io::Error) -> HistoryStorageError {
    HistoryStorageError(error.to_string())
}

fn sqlite_error(error: rusqlite::Error) -> HistoryStorageError {
    HistoryStorageError(error.to_string())
}

#[cfg(test)]
mod tests;
