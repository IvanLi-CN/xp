use super::*;
use std::os::fd::AsRawFd;

pub(super) const EXTERNAL_REPOSITORY_STARTUP_FAILURE: &str =
    "external repository history startup preparation failed: ";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HistoryStorageMode {
    Sqlite,
    DegradedJson,
    Unavailable,
}

pub(super) fn sqlite_connection(backend: &mut Backend) -> Result<Option<&mut Connection>> {
    match backend {
        Backend::Sqlite(connection) => Ok(Some(connection)),
        Backend::Json => Ok(None),
        Backend::Unavailable(error) => Err(error.clone()),
    }
}

pub(super) fn open_backend(data_dir: &Path) -> Backend {
    let db_path = data_dir.join(SQLITE_FILE);
    match fs::metadata(&db_path) {
        Ok(metadata) => match open_sqlite(data_dir) {
            Ok(connection) => match repository_history_is_external(&connection) {
                Ok(true) => Backend::Sqlite(connection),
                Ok(false) if json_fallback_path(data_dir).exists() => Backend::Json,
                Ok(false) => Backend::Sqlite(connection),
                Err(error) => {
                    let marker_error = format!("cannot inspect repository history marker: {error}");
                    let error = HistoryStorageError(format!(
                        "{EXTERNAL_REPOSITORY_STARTUP_FAILURE}{marker_error}"
                    ));
                    warn!(
                        error = %error,
                        path = %db_path.display(),
                        history_storage_mode = "unavailable",
                        "cannot inspect repository history marker; refusing JSON fallback"
                    );
                    Backend::Unavailable(error)
                }
            },
            Err(error) if metadata.is_file() => {
                let error = if is_external_repository_startup_failure(&error) {
                    error
                } else {
                    HistoryStorageError(format!("{EXTERNAL_REPOSITORY_STARTUP_FAILURE}{error}"))
                };
                warn!(
                    error = %error,
                    path = %db_path.display(),
                    history_storage_mode = "unavailable",
                    "existing history SQLite failed startup preparation; refusing JSON fallback"
                );
                Backend::Unavailable(error)
            }
            Err(error) => {
                warn!(
                    error = %error,
                    path = %db_path.display(),
                    history_storage_mode = "degraded_json",
                    "history storage degraded; continuing with JSON snapshots"
                );
                Backend::Json
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if json_fallback_path(data_dir).exists() {
                Backend::Json
            } else {
                match open_sqlite(data_dir) {
                    Ok(connection) => Backend::Sqlite(connection),
                    Err(error) => {
                        warn!(
                            error = %error,
                            path = %db_path.display(),
                            history_storage_mode = "degraded_json",
                            "history storage degraded; continuing with JSON snapshots"
                        );
                        Backend::Json
                    }
                }
            }
        }
        Err(error) => {
            let error = HistoryStorageError(format!(
                "{EXTERNAL_REPOSITORY_STARTUP_FAILURE}cannot inspect existing history SQLite: {}",
                io_error(error)
            ));
            warn!(
                error = %error,
                path = %db_path.display(),
                history_storage_mode = "unavailable",
                "cannot inspect existing history SQLite; refusing JSON fallback"
            );
            Backend::Unavailable(error)
        }
    }
}

pub(super) fn open_sqlite(data_dir: &Path) -> Result<Connection> {
    fs::create_dir_all(data_dir).map_err(io_error)?;
    let db_path = data_dir.join(SQLITE_FILE);
    match fs::metadata(&db_path) {
        Ok(metadata) if metadata.is_file() => {
            let mut connection = Connection::open(&db_path)
                .map_err(|error| external_startup_error(sqlite_error(error)))?;
            if let Err(error) =
                configure_runtime(&connection).and_then(|()| ensure_schema(&mut connection))
            {
                warn!(
                    error = %error,
                    path = %db_path.display(),
                    history_storage_mode = "unavailable",
                    "preserving existing SQLite repository history after startup \
                     preparation failure"
                );
                return Err(external_startup_error(error));
            }
            return Ok(connection);
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let _migration_lock = MigrationLock::acquire(data_dir)?;
            if fs::metadata(&db_path).is_ok() {
                drop(_migration_lock);
                return open_sqlite(data_dir);
            }
            migrate_json_snapshots(data_dir, &db_path)?;
            let mut connection = Connection::open(db_path)
                .map_err(|error| external_startup_error(sqlite_error(error)))?;
            configure_runtime(&connection).map_err(external_startup_error)?;
            ensure_schema(&mut connection).map_err(external_startup_error)?;
            return Ok(connection);
        }
        Err(error) => {
            return Err(HistoryStorageError(format!(
                "{EXTERNAL_REPOSITORY_STARTUP_FAILURE}cannot inspect existing history SQLite: {}",
                io_error(error)
            )));
        }
    }

    // A non-file path (for example, a directory left by a failed migration) is not a durable
    // SQLite database. Let the normal JSON degradation path handle that legacy collision.
    let mut connection = Connection::open(&db_path).map_err(sqlite_error)?;
    configure_runtime(&connection)?;
    ensure_schema(&mut connection)?;
    Ok(connection)
}

fn external_startup_error(error: HistoryStorageError) -> HistoryStorageError {
    HistoryStorageError(format!("{EXTERNAL_REPOSITORY_STARTUP_FAILURE}{error}"))
}

struct MigrationLock {
    _file: fs::File,
}

impl MigrationLock {
    fn acquire(data_dir: &Path) -> Result<Self> {
        let path = data_dir.join("history.sqlite3.migration.lock");
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(io_error)?;
        loop {
            let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if result == 0 {
                return Ok(Self { _file: file });
            }
            let error = io::Error::last_os_error();
            let lock_busy = matches!(error.raw_os_error(), Some(code)
                if code == libc::EWOULDBLOCK || code == libc::EAGAIN);
            if !lock_busy {
                return Err(io_error(error));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

pub(super) fn is_external_repository_startup_failure(error: &HistoryStorageError) -> bool {
    error.0.starts_with(EXTERNAL_REPOSITORY_STARTUP_FAILURE)
}

pub(super) fn repository_history_is_external(connection: &Connection) -> Result<bool> {
    let Some(payload) = read_sqlite(connection, REPOSITORY_REPLICA_KEY)? else {
        return Ok(false);
    };
    let snapshot = serde_json::from_slice::<serde_json::Value>(&payload).map_err(|error| {
        HistoryStorageError(format!("invalid repository history snapshot: {error}"))
    })?;
    let Some(external_history) = snapshot.get("external_history") else {
        return Ok(false);
    };
    external_history.as_bool().ok_or_else(|| {
        HistoryStorageError("repository history external_history marker is not boolean".to_owned())
    })
}

#[cfg(test)]
thread_local! {
    static FAIL_NEXT_SEGMENT_KEYSET_INDEX: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(super) fn fail_next_segment_keyset_index_for_test() {
    FAIL_NEXT_SEGMENT_KEYSET_INDEX.with(|failure| failure.set(true));
}

#[cfg(test)]
pub(super) fn take_segment_keyset_index_failure_for_test() -> bool {
    FAIL_NEXT_SEGMENT_KEYSET_INDEX.with(|failure| failure.replace(false))
}
