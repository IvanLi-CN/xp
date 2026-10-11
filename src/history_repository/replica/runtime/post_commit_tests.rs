use super::*;

#[test]
fn maintenance_failure_keeps_runtime_and_sqlite_committed() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let mut runtime = RepositoryReplicaRuntime::load(storage.clone()).expect("runtime");
    storage.set_maintenance_failure_for_test(true);

    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &segment(&key, 0, vec![record(b"committed", false)], None)
                .wire_bytes()
                .expect("wire"),
            11,
        )
        .expect("maintenance failure occurs after the durable commit");
    assert_eq!(storage.repository_history_record_count().expect("count"), 1);
    assert_eq!(
        runtime.runtime_status(12).expect("status").storage_mode,
        "sqlite_degraded"
    );

    storage.set_maintenance_failure_for_test(false);
    let restored = RepositoryReplicaRuntime::load(storage).expect("reload committed runtime");
    assert_eq!(
        restored
            .storage
            .repository_history_record_count()
            .expect("reloaded count"),
        1
    );
    assert!(restored.snapshot.receiver.is_some());
}

#[test]
fn summary_repair_diagnostic_survives_restart_with_pending_checkpoint() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let mut runtime = RepositoryReplicaRuntime::load(storage.clone()).expect("runtime");

    runtime
        .update_initial_peer_summary_checkpoint(
            "peer-101",
            Some("summary-cursor".to_owned()),
            vec!["segment-a".to_owned(), "segment-b".to_owned()],
            Some("next-cursor".to_owned()),
            false,
            false,
        )
        .expect("pending summary checkpoint");
    runtime
        .record_summary_repair_attempt("peer-101", 100)
        .expect("record attempt");
    runtime
        .record_summary_repair_result(
            "peer-101",
            101,
            SummaryRepairResult::NoProgress,
            Some("no_progress".to_owned()),
            Some(161),
        )
        .expect("record result");

    let status = runtime.runtime_status(102).expect("status");
    let diagnostic = status
        .summary_repairs
        .get("peer-101")
        .expect("summary repair diagnostic");
    assert_eq!(diagnostic.pending_count, 2);
    assert_eq!(diagnostic.last_attempt_at, Some(100));
    assert_eq!(diagnostic.last_result, SummaryRepairResult::NoProgress);
    assert_eq!(diagnostic.last_error_code.as_deref(), Some("no_progress"));
    assert_eq!(diagnostic.retry_after, Some(161));
    assert_eq!(diagnostic.attempt_count, 1);

    let mut restored = RepositoryReplicaRuntime::load(storage).expect("reload runtime");
    let restored_status = restored.runtime_status(103).expect("restored status");
    let restored_diagnostic = restored_status
        .summary_repairs
        .get("peer-101")
        .expect("restored summary repair diagnostic");
    assert_eq!(restored_diagnostic.pending_count, 2);
    assert_eq!(
        restored_diagnostic.last_result,
        SummaryRepairResult::NoProgress
    );
}

#[test]
fn summary_repair_transition_rolls_back_checkpoint_when_diagnostic_write_fails() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let mut runtime = RepositoryReplicaRuntime::load(storage.clone()).expect("runtime");
    runtime
        .update_initial_peer_summary_checkpoint(
            "peer-101",
            Some("before".to_owned()),
            vec!["segment-a".to_owned()],
            Some("next".to_owned()),
            false,
            false,
        )
        .expect("initial checkpoint");
    let before = runtime.initial_peer_backfill_checkpoint("peer-101");
    let before_diagnostic = runtime
        .snapshot
        .summary_repair_diagnostics
        .get("peer-101")
        .cloned();

    storage
        .set_query_only_for_test(true)
        .expect("enable query-only storage");
    assert!(
        runtime
            .persist_summary_repair_transition(
                "peer-101",
                SummaryRepairDiagnosticUpdate {
                    now_unix_seconds: 101,
                    result: SummaryRepairResult::Progress,
                    error_code: None,
                    retry_after: None,
                },
                |runtime| {
                    runtime.mark_history_truncated_in_memory();
                    runtime.update_initial_peer_summary_checkpoint_in_memory(
                        "peer-101",
                        Some("after".to_owned()),
                        Vec::new(),
                        None,
                        true,
                        false,
                    );
                    Ok(())
                },
            )
            .is_err()
    );
    assert_eq!(runtime.initial_peer_backfill_checkpoint("peer-101"), before);
    assert_eq!(
        runtime
            .snapshot
            .summary_repair_diagnostics
            .get("peer-101")
            .cloned(),
        before_diagnostic
    );

    storage
        .set_query_only_for_test(false)
        .expect("disable query-only storage");
    runtime
        .persist_summary_repair_transition(
            "peer-101",
            SummaryRepairDiagnosticUpdate {
                now_unix_seconds: 103,
                result: SummaryRepairResult::Progress,
                error_code: None,
                retry_after: None,
            },
            |runtime| {
                runtime.mark_history_truncated_in_memory();
                runtime.update_initial_peer_summary_checkpoint_in_memory(
                    "peer-101",
                    Some("after".to_owned()),
                    Vec::new(),
                    None,
                    true,
                    false,
                );
                Ok(())
            },
        )
        .expect("atomic summary repair transition");
    drop(runtime);
    let mut restored = RepositoryReplicaRuntime::load(storage).expect("reload runtime");
    let checkpoint = restored
        .initial_peer_backfill_checkpoint("peer-101")
        .expect("checkpoint");
    assert!(checkpoint.summary_complete);
    assert_eq!(checkpoint.summary_cursor.as_deref(), Some("after"));
    assert!(restored.history_truncated());
    assert_eq!(
        restored
            .runtime_status(104)
            .expect("status")
            .summary_repairs
            .get("peer-101")
            .expect("diagnostic")
            .last_result,
        SummaryRepairResult::Progress
    );
}
