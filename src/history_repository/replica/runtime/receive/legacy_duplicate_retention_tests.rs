use super::*;

fn assert_duplicate_does_not_restore_records(expired: bool) {
    let directory = tempfile::tempdir().unwrap();
    let (mut runtime, chain) = ahead_receiver(directory.path(), 239, false, false);
    stage_evidence(&mut runtime, &chain);
    let source = identity(&signing_key());
    let now = 12 + TOMBSTONE_HORIZON_SECONDS;
    if !expired {
        let deletion = CanonicalSegment::new(
            "cluster-a",
            Cursor::new("node-a", 7, "tombstone", 0).unwrap(),
            vec![record(b"96", true)],
            None,
            12,
            12,
        )
        .unwrap()
        .sign(&signing_key())
        .unwrap();
        runtime
            .receive_wire("cluster-a", &source, &deletion.wire_bytes().unwrap(), 12)
            .unwrap();
        assert!(
            !runtime
                .storage
                .repository_history_records(None, None, None, 0, 1000)
                .unwrap()
                .iter()
                .any(|row| !row.tombstone && row.stream == "runtime" && row.sequence == 96)
        );
    }
    // Keep the receiver continuously fresh through an actual delayed heartbeat receipt.
    // This avoids bypassing stale-rebuild authorization while advancing real retention time.
    let warm = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "freshness", 0).unwrap(),
        vec![record(b"old-heartbeat", false)],
        None,
        11,
        11,
    )
    .unwrap()
    .sign(&signing_key())
    .unwrap();
    runtime
        .receive_wire("cluster-a", &source, &warm.wire_bytes().unwrap(), 12)
        .unwrap();
    // A real later receipt runs the production tombstone expiry and retention transactions.
    let fresh = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "freshness", 1).unwrap(),
        vec![record(b"heartbeat", false)],
        Some(warm.segment_hash().unwrap()),
        now,
        now,
    )
    .unwrap()
    .sign(&signing_key())
    .unwrap();
    runtime
        .receive_wire("cluster-a", &source, &fresh.wire_bytes().unwrap(), now)
        .unwrap();
    runtime.prune_retention(now).unwrap();
    let before = runtime
        .storage
        .repository_history_records(None, None, None, 0, 1000)
        .unwrap();
    assert!(
        !before
            .iter()
            .any(|row| !row.tombstone && row.stream == "runtime" && row.sequence == 96)
    );
    if expired {
        assert_eq!(
            before.len(),
            1,
            "only the fresh heartbeat survives real retention"
        );
    }
    assert!(
        serde_json::to_value(runtime.tombstones.checkpoint()).unwrap()["entries"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let database = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    database
        .execute("DELETE FROM repository_history_segments", [])
        .unwrap();
    drop(database);
    let before_receiver =
        serde_json::to_value(runtime.receiver.as_ref().unwrap().checkpoint().unwrap()).unwrap();
    let before_source =
        serde_json::to_value(runtime.snapshot_for_persistence()).unwrap()["local_source"].clone();
    drop(runtime);
    let mut runtime = load(directory.path());
    let mut complete = false;
    for _ in 0..48 {
        let state = runtime
            .legacy_original_recovery_state("cluster-a", &source, now)
            .unwrap();
        complete = runtime
            .legacy_recovery_apply_step(
                "cluster-a",
                &source,
                &state,
                now,
                &["local".into()],
                "local",
            )
            .unwrap();
        let after = runtime
            .storage
            .repository_history_records(None, None, None, 0, 1000)
            .unwrap();
        assert_eq!(
            after.len(),
            before.len(),
            "historical duplicate must not resurrect ordinary records"
        );
        assert_eq!(
            after, before,
            "retained rows and their provenance must remain unchanged"
        );
        drop(runtime);
        runtime = load(directory.path());
        if complete {
            break;
        }
    }
    assert!(
        complete,
        "a proven receipt must still ACK the original outbox page"
    );
    assert_eq!(
        runtime.storage.source_delivery_journal().unwrap().len(),
        143
    );
    assert_eq!(
        serde_json::to_value(runtime.receiver.as_ref().unwrap().checkpoint().unwrap()).unwrap(),
        before_receiver
    );
    assert_eq!(
        serde_json::to_value(runtime.snapshot_for_persistence()).unwrap()["local_source"],
        before_source
    );
    assert!(runtime.snapshot.gaps.is_empty());
}

#[test]
fn proven_duplicate_keeps_retention_expired_records_expired_after_id_eviction_and_restart() {
    assert_duplicate_does_not_restore_records(true);
}

#[test]
fn proven_duplicate_keeps_deleted_records_deleted_after_ledger_and_id_expiry_and_restart() {
    assert_duplicate_does_not_restore_records(false);
}
