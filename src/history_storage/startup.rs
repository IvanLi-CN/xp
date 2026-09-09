use super::*;

const EXTERNAL_REPOSITORY_STARTUP_FAILURE: &str =
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

pub(super) fn open_sqlite(data_dir: &Path) -> Result<Connection> {
    fs::create_dir_all(data_dir).map_err(io_error)?;
    let db_path = data_dir.join(SQLITE_FILE);
    if db_path.exists() {
        let mut connection = Connection::open(&db_path).map_err(sqlite_error)?;
        if let Err(error) =
            configure_runtime(&connection).and_then(|()| ensure_schema(&mut connection))
        {
            match repository_history_is_external(&connection) {
                Ok(true) | Err(_) => {
                    warn!(
                        error = %error,
                        path = %db_path.display(),
                        history_storage_mode = "unavailable",
                        "preserving external SQLite repository history after startup \
                         preparation failure"
                    );
                    return Err(HistoryStorageError(format!(
                        "{EXTERNAL_REPOSITORY_STARTUP_FAILURE}{error}"
                    )));
                }
                Ok(false) => {}
            }
            return Err(error);
        }
        return Ok(connection);
    }

    migrate_json_snapshots(data_dir, &db_path)?;
    let mut connection = Connection::open(db_path).map_err(sqlite_error)?;
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
