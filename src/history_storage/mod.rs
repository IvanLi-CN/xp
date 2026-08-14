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

enum Backend {
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
        let storage = Self { data_dir, backend };
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
                    read_json(source_path(&self.data_dir, key))
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
                    write_json(source_path(&self.data_dir, key), payload)
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

    fn lock_backend(&self) -> std::sync::MutexGuard<'_, Backend> {
        self.backend
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            ",
        )
        .map_err(sqlite_error)
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
        .map_err(sqlite_error)?;
    transaction.commit().map_err(sqlite_error)?;

    // Both operations are page-bounded and avoid a blocking full checkpoint or VACUUM.
    connection
        .pragma_update(None, "wal_autocheckpoint", CHECKPOINT_PAGES)
        .map_err(sqlite_error)?;
    connection
        .execute_batch(&format!("PRAGMA incremental_vacuum({VACUUM_PAGES})"))
        .map_err(sqlite_error)?;
    Ok(())
}

fn switch_to_json(backend: &mut Backend, data_dir: &Path) {
    let Backend::Sqlite(connection) = backend else {
        return;
    };
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

fn source_path(data_dir: &Path, key: &str) -> PathBuf {
    SOURCES
        .iter()
        .find(|source| source.key == key)
        .map(|source| source.path(data_dir))
        .unwrap_or_else(|| data_dir.join(format!("{key}.json")))
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
            "INSERT INTO history_storage_meta (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .map_err(sqlite_error)?;
    Ok(())
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
mod tests {
    use std::{fs, time::Duration};

    use super::{
        Backend, HistoryStorage, HistoryStorageMode, INBOUND_IP_USAGE_KEY, MESH_TELEMETRY_KEY,
        NODE_HISTORY_KEY, REPOSITORY_REPLICA_KEY, STATE_KEY, TCP_CONNECTION_USAGE_KEY, USAGE_KEY,
    };

    #[test]
    fn migrates_legacy_snapshots_into_sqlite_without_rewriting_json() {
        let temporary = tempfile::tempdir().unwrap();
        let snapshots = [
            ("state.json", STATE_KEY, b"state".as_slice()),
            ("usage.json", USAGE_KEY, b"usage".as_slice()),
            (
                "inbound_ip_usage.json",
                INBOUND_IP_USAGE_KEY,
                b"inbound-ip".as_slice(),
            ),
            (
                "tcp_connection_usage.json",
                TCP_CONNECTION_USAGE_KEY,
                b"tcp-connections".as_slice(),
            ),
            (
                "node_history_cache.json",
                NODE_HISTORY_KEY,
                b"node-history".as_slice(),
            ),
            (
                "mesh/telemetry.json",
                MESH_TELEMETRY_KEY,
                b"mesh-telemetry".as_slice(),
            ),
            (
                "history/repository_replica.json",
                REPOSITORY_REPLICA_KEY,
                b"repository-replica".as_slice(),
            ),
        ];
        for (relative_path, _, payload) in snapshots {
            let path = temporary.path().join(relative_path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, payload).unwrap();
        }

        let storage = HistoryStorage::open(temporary.path());

        assert!(storage.is_sqlite());
        for (relative_path, key, payload) in snapshots {
            assert_eq!(storage.read(key).unwrap(), Some(payload.to_vec()));
            assert_eq!(
                fs::read(temporary.path().join(relative_path)).unwrap(),
                payload
            );
        }
    }

    #[test]
    fn failed_migration_keeps_json_as_the_only_active_store() {
        let temporary = tempfile::tempdir().unwrap();
        fs::create_dir(temporary.path().join("history.sqlite3")).unwrap();
        let legacy_path = temporary.path().join("state.json");
        fs::write(&legacy_path, b"old").unwrap();

        let storage = HistoryStorage::open(temporary.path());
        storage.write(STATE_KEY, b"new").unwrap();

        assert!(!storage.is_sqlite());
        assert_eq!(storage.mode(), HistoryStorageMode::DegradedJson);
        assert_eq!(fs::read(&legacy_path).unwrap(), b"new");
    }

    #[test]
    fn failed_sqlite_write_restores_json_then_continues_with_json_only() {
        let temporary = tempfile::tempdir().unwrap();
        let legacy_path = temporary.path().join("state.json");
        fs::write(&legacy_path, b"before").unwrap();
        let storage = HistoryStorage::open(temporary.path());
        set_query_only(&storage);

        storage.write(STATE_KEY, b"after").unwrap();

        assert!(!storage.is_sqlite());
        assert_eq!(fs::read(&legacy_path).unwrap(), b"after");
        storage.write(STATE_KEY, b"final").unwrap();
        assert_eq!(fs::read(&legacy_path).unwrap(), b"final");
    }

    #[test]
    fn sqlite_write_failure_keeps_json_as_the_only_store_after_restart() {
        let temporary = tempfile::tempdir().unwrap();
        let legacy_path = temporary.path().join("state.json");
        fs::write(&legacy_path, b"before").unwrap();
        let storage = HistoryStorage::open(temporary.path());
        set_query_only(&storage);
        storage.write(STATE_KEY, b"after").unwrap();
        drop(storage);

        let restarted = HistoryStorage::open(temporary.path());

        assert!(!restarted.is_sqlite());
        assert_eq!(restarted.read(STATE_KEY).unwrap(), Some(b"after".to_vec()));
    }

    #[test]
    fn handles_for_one_data_dir_share_the_json_fallback() {
        let temporary = tempfile::tempdir().unwrap();
        let first = HistoryStorage::open(temporary.path());
        let second = HistoryStorage::open(temporary.path());
        set_query_only(&first);

        first.write(STATE_KEY, b"fallback").unwrap();

        assert!(!second.is_sqlite());
        assert_eq!(second.read(STATE_KEY).unwrap(), Some(b"fallback".to_vec()));
    }

    #[test]
    fn restart_uses_the_committed_migration_instead_of_reimporting_json() {
        let temporary = tempfile::tempdir().unwrap();
        let legacy_path = temporary.path().join("state.json");
        fs::write(&legacy_path, b"first").unwrap();

        let storage = HistoryStorage::open(temporary.path());
        assert_eq!(storage.read(STATE_KEY).unwrap(), Some(b"first".to_vec()));
        drop(storage);
        fs::write(&legacy_path, b"newer-json").unwrap();

        let restarted = HistoryStorage::open(temporary.path());
        assert_eq!(restarted.read(STATE_KEY).unwrap(), Some(b"first".to_vec()));
    }

    #[test]
    fn sqlite_write_does_not_double_write_the_json_backup() {
        let temporary = tempfile::tempdir().unwrap();
        let legacy_path = temporary.path().join("state.json");
        fs::write(&legacy_path, b"before").unwrap();
        let storage = HistoryStorage::open(temporary.path());

        storage.write(STATE_KEY, b"after").unwrap();

        assert_eq!(storage.read(STATE_KEY).unwrap(), Some(b"after".to_vec()));
        assert_eq!(fs::read(&legacy_path).unwrap(), b"before");
    }

    #[test]
    fn removes_migrated_json_backups_after_thirty_days() {
        let temporary = tempfile::tempdir().unwrap();
        let legacy_path = temporary.path().join("state.json");
        fs::write(&legacy_path, b"before").unwrap();
        let storage = HistoryStorage::open(temporary.path());

        storage.cleanup_expired_backups_at(
            std::time::SystemTime::now() + Duration::from_secs(31 * 24 * 60 * 60),
        );

        assert!(!legacy_path.exists());
    }

    #[test]
    fn configures_wal_bounded_checkpoint_and_incremental_vacuum() {
        let temporary = tempfile::tempdir().unwrap();
        let storage = HistoryStorage::open(temporary.path());
        storage
            .write(NODE_HISTORY_KEY, &vec![7; 256 * 1024])
            .unwrap();
        let pages_before_shrink = sqlite_pragma(&storage, "page_count");
        storage.write(NODE_HISTORY_KEY, b"small").unwrap();
        storage.write(MESH_TELEMETRY_KEY, b"mesh").unwrap();
        storage.write(INBOUND_IP_USAGE_KEY, b"ip").unwrap();

        let journal_mode = sqlite_text_pragma(&storage, "journal_mode");
        let auto_vacuum = sqlite_pragma(&storage, "auto_vacuum");
        let auto_checkpoint = sqlite_pragma(&storage, "wal_autocheckpoint");
        let pages_after_shrink = sqlite_pragma(&storage, "page_count");
        let free_pages = sqlite_pragma(&storage, "freelist_count");

        assert_eq!(journal_mode, "wal");
        assert_eq!(auto_vacuum, 2);
        assert_eq!(auto_checkpoint, 64);
        assert!(pages_after_shrink <= pages_before_shrink);
        assert!(free_pages < 64);
    }

    fn sqlite_pragma(storage: &HistoryStorage, pragma: &str) -> i64 {
        let backend = storage.lock_backend();
        let Backend::Sqlite(connection) = &*backend else {
            panic!("test storage should use SQLite");
        };
        connection
            .query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0))
            .unwrap()
    }

    fn sqlite_text_pragma(storage: &HistoryStorage, pragma: &str) -> String {
        let backend = storage.lock_backend();
        let Backend::Sqlite(connection) = &*backend else {
            panic!("test storage should use SQLite");
        };
        connection
            .query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0))
            .unwrap()
    }

    fn set_query_only(storage: &HistoryStorage) {
        let backend = storage.lock_backend();
        let Backend::Sqlite(connection) = &*backend else {
            panic!("test storage should use SQLite");
        };
        connection.pragma_update(None, "query_only", "ON").unwrap();
    }
}
