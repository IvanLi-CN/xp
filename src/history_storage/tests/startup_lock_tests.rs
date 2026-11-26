use super::*;
use crate::state::history_storage::repository::query_sql;

/// Startup must not build the record page index.
///
/// On a multi-gigabyte repository the build takes over a minute, while container readiness waits
/// only twenty seconds. If startup owned the build, every restart would roll it back and it would
/// never commit. Correctness never depends on it, so preparation returns immediately and a
/// background worker owns the build.
#[test]
fn startup_never_builds_the_record_page_index() {
    let temporary = tempfile::tempdir().unwrap();
    drop(HistoryStorage::open(temporary.path()));

    let storage = HistoryStorage::open(temporary.path());
    assert!(storage.is_sqlite());
    assert!(
        !storage.repository_page_index_present().unwrap(),
        "startup preparation must leave the page index to the background worker"
    );
}

/// The background build waits for a concurrent writer instead of failing.
///
/// Building the index needs a write lock. Without a busy timeout a single concurrent writer makes
/// preparation fail outright, which is indistinguishable from a corrupt or unusable database.
#[test]
fn background_page_index_build_waits_for_a_concurrent_writer() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join(SQLITE_FILE);
    drop(HistoryStorage::open(temporary.path()));

    // Hold the write lock from another process: SQLite does not run the busy handler against a
    // lock its own process already holds, so an in-process holder would test the wrong thing.
    let mut lock_holder = std::process::Command::new("sqlite3")
        .arg(&path)
        .arg(
            "PRAGMA journal_mode=WAL;\
             BEGIN IMMEDIATE;\
             INSERT INTO history_snapshots(key,payload,updated_at) VALUES('held','x',1)\
               ON CONFLICT(key) DO UPDATE SET payload='y';\
             SELECT 'held';",
        )
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn an external lock holder");
    let held = {
        let mut buf = [0u8; 1024];
        use std::io::Read as _;
        if let Some(out) = lock_holder.stdout.as_mut() {
            let _ = out.read(&mut buf);
        }
        String::from_utf8_lossy(&buf).trim().to_string()
    };
    assert!(
        held.contains("held"),
        "the external holder must take the write lock, saw: {held:?}"
    );

    let releaser = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(600));
        let _ = lock_holder.kill();
        let _ = lock_holder.wait();
    });

    let storage = HistoryStorage::open(temporary.path());
    assert!(
        storage.build_repository_page_index().unwrap(),
        "the build must complete once the writer releases"
    );
    releaser.join().unwrap();
    assert!(storage.repository_page_index_present().unwrap());
}

/// Building twice is idempotent and the superseded index is removed.
#[test]
fn page_index_build_is_idempotent_and_drops_the_superseded_index() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(temporary.path());
    assert!(storage.build_repository_page_index().unwrap());
    assert!(
        !storage.build_repository_page_index().unwrap(),
        "a second build must report that nothing changed"
    );

    let backend = storage.lock_backend();
    let Backend::Sqlite(connection) = &*backend else {
        panic!("storage should be SQLite");
    };
    assert!(
        !query_sql::page_index_present(connection).unwrap()
            || connection
                .query_row(
                    "SELECT count(*) FROM sqlite_master
                      WHERE type = 'index' AND name = 'repository_history_records_page'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap()
                == 0,
        "the superseded page index must be dropped"
    );
}
