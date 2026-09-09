use super::*;

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
            let mut connection = Connection::open(&db_path).map_err(|error| {
                HistoryStorageError(format!(
                    "{EXTERNAL_REPOSITORY_STARTUP_FAILURE}cannot open existing history SQLite: {}",
                    sqlite_error(error)
                ))
            })?;
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
                return Err(HistoryStorageError(format!(
                    "{EXTERNAL_REPOSITORY_STARTUP_FAILURE}{error}"
                )));
            }
            return Ok(connection);
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            migrate_json_snapshots(data_dir, &db_path)?;
            let mut connection = Connection::open(db_path).map_err(sqlite_error)?;
            configure_runtime(&connection)?;
            ensure_schema(&mut connection)?;
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
    Ok(snapshot
        .get("external_history")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false))
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
