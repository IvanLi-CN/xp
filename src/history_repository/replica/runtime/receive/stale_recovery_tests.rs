use super::*;
use crate::state::history_repository::replica::runtime::tests::{
    identity, load, record, segment, signing_key,
};
use crate::state::history_storage::SourceDeliveryJournalRow;

fn pending(runtime: &mut RepositoryReplicaRuntime) -> RepositoryNodeIdentity {
    let key = signing_key();
    let identity = identity(&key);
    let original = segment(&key, 0, vec![record(b"pending", false)], None);
    let wire = original.wire_bytes().unwrap();
    let id = hex::encode(Sha256::digest(&wire));
    runtime
        .storage
        .append_source_delivery_journal(&[SourceDeliveryJournalRow {
            id: id.clone(),
            stream: "runtime".into(),
            closed_at_unix_seconds: 11,
            identity: identity.clone(),
            wire,
        }])
        .unwrap();
    let mut value = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    value["local_source"]["source_epoch"] = serde_json::json!(7);
    value["local_source"]["epoch"] = serde_json::json!(7);
    value["local_source"]["streams"] = serde_json::json!({"runtime": {
        "next_sequence": 1, "previous_segment_hash": original.segment_hash().unwrap(), "pending": []
    }});
    value["local_history_backfill_inflight"] = serde_json::json!({
        "segment_ids": [id], "page_cursor": null, "completed": true
    });
    runtime.snapshot = serde_json::from_value(value).unwrap();
    runtime.persist_control_state().unwrap();
    identity
}

#[test]
fn source_deletion_page_rolls_back_wire_and_cursor_on_sqlite_failure() {
    let dir = tempfile::tempdir().unwrap();
    let mut runtime = load(dir.path());
    let identity = pending(&mut runtime);
    let peers = vec![(identity.node_id().as_str().to_owned(), [3; 32])];
    let generation = runtime
        .begin_retained_stale_recovery("cluster-a", &identity, &peers, 12)
        .unwrap();
    let prior = runtime
        .retained_stale_recovery_progress("cluster-a", &identity, &peers)
        .unwrap();
    let connection = rusqlite::Connection::open(dir.path().join("history.sqlite3")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER fail_source_stale BEFORE INSERT ON repository_history_records
         WHEN NEW.is_tombstone = 1 BEGIN SELECT RAISE(ABORT, 'failure'); END;",
        )
        .unwrap();
    let deletion = SyncRecord::new(
        "node-a",
        "node-a",
        "runtime.v1",
        1,
        b"node-history:node:node-a:".to_vec(),
        vec![],
        true,
    );
    let id = identity.node_id().as_str();
    let call = |runtime: &mut RepositoryReplicaRuntime| {
        runtime.import_retained_stale_source_deletions(
            "cluster-a",
            &identity,
            &signing_key(),
            &peers,
            generation,
            id,
            None,
            None,
            "v2:1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            vec![(12, deletion.clone())],
            12,
        )
    };
    assert!(call(&mut runtime).is_err());
    assert_eq!(
        prior,
        runtime
            .retained_stale_recovery_progress("cluster-a", &identity, &peers)
            .unwrap()
    );
    assert!(
        runtime
            .storage
            .repository_history_records(None, None, None, 0, 10)
            .unwrap()
            .is_empty()
    );
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 1);
    connection
        .execute_batch("DROP TRIGGER fail_source_stale")
        .unwrap();
    call(&mut runtime).unwrap();
    let tombstones: u64 = connection
        .query_row(
            "SELECT COUNT(*) FROM repository_history_records WHERE is_tombstone = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tombstones, 1);
    assert_eq!(
        runtime.storage.repository_history_segment_count().unwrap(),
        1
    );
    let restored = load(dir.path());
    assert!(
        restored
            .retained_stale_recovery_progress("cluster-a", &identity, &peers)
            .unwrap()
            .unwrap()
            .2[0]
            .2
    );
}

#[test]
fn source_deletion_rejects_exact_and_accepts_empty_terminal_page() {
    let dir = tempfile::tempdir().unwrap();
    let mut runtime = load(dir.path());
    let identity = pending(&mut runtime);
    let id = identity.node_id().as_str();
    let peers = vec![(id.to_owned(), [5; 32])];
    let generation = runtime
        .begin_retained_stale_recovery("cluster-a", &identity, &peers, 12)
        .unwrap();
    let exact = SyncRecord::new(
        id,
        id,
        "traffic.v1",
        1,
        b"node-history:user:alice:current".to_vec(),
        vec![],
        true,
    );
    let snapshot = "v2:1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    assert!(
        runtime
            .import_retained_stale_source_deletions(
                "cluster-a",
                &identity,
                &signing_key(),
                &peers,
                generation,
                id,
                None,
                None,
                snapshot,
                vec![(12, exact)],
                12,
            )
            .is_err()
    );
    runtime
        .import_retained_stale_source_deletions(
            "cluster-a",
            &identity,
            &signing_key(),
            &peers,
            generation,
            id,
            None,
            None,
            snapshot,
            vec![],
            12,
        )
        .unwrap();
    assert!(
        runtime
            .retained_stale_recovery_progress("cluster-a", &identity, &peers)
            .unwrap()
            .unwrap()
            .2[0]
            .2
    );
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 1);
}

#[test]
fn stale_generation_is_bound_to_page_and_ready_set_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let mut runtime = load(dir.path());
    let source = pending(&mut runtime);
    let peers = vec![("node-b".to_owned(), [1; 32])];
    let generation = runtime
        .begin_retained_stale_recovery("cluster-a", &source, &peers, 12)
        .unwrap();
    assert!(
        !runtime
            .retained_stale_page_authorized("cluster-a", &source, &peers, generation, 12)
            .unwrap()
    );
    runtime
        .import_retained_stale_deletions(
            "cluster-a",
            &source,
            &peers,
            generation,
            "node-b",
            None,
            None,
            true,
            vec![],
            12,
            "node-a",
        )
        .unwrap();
    runtime
        .seal_retained_stale_recovery("cluster-a", &source, &peers, generation, 12)
        .unwrap();
    drop(runtime);
    let runtime = load(dir.path());
    assert!(
        runtime
            .retained_stale_page_authorized("cluster-a", &source, &peers, generation, 12)
            .unwrap()
    );
    assert!(
        !runtime
            .retained_stale_page_authorized(
                "cluster-a",
                &source,
                &[("node-b".into(), [2; 32])],
                generation,
                12,
            )
            .unwrap()
    );
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 1);
}

#[test]
fn stale_generation_must_rescan_expired_deletion_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let mut runtime = load(dir.path());
    let source = pending(&mut runtime);
    let peers = vec![("node-b".to_owned(), [1; 32])];
    let generation = runtime
        .begin_retained_stale_recovery("cluster-a", &source, &peers, 12)
        .unwrap();
    let deleted = RepositoryTieredBackfillRecord {
        observed_at_unix_seconds: 11,
        source_node_id: "node-c".into(),
        source_epoch: 7,
        stream: "tombstone".into(),
        sequence: 0,
        subject_node_id: "subject-a".into(),
        observer_node_id: "node-a".into(),
        schema_id: "runtime.v1".into(),
        schema_version: 1,
        record_key: b"pending".to_vec(),
        payload: Vec::new(),
        tombstone: true,
    };
    runtime
        .import_retained_stale_deletions(
            "cluster-a",
            &source,
            &peers,
            generation,
            "node-b",
            None,
            None,
            true,
            vec![deleted],
            12,
            "node-a",
        )
        .unwrap();
    runtime
        .seal_retained_stale_recovery("cluster-a", &source, &peers, generation, 12)
        .unwrap();
    let expired = 12 + TOMBSTONE_HORIZON_SECONDS;
    assert!(
        !runtime
            .retained_stale_page_authorized("cluster-a", &source, &peers, generation, expired)
            .unwrap()
    );
    let next = runtime
        .begin_retained_stale_recovery("cluster-a", &source, &peers, expired)
        .unwrap();
    assert_eq!(next, generation + 1);
    assert!(
        !runtime
            .retained_stale_recovery_progress("cluster-a", &source, &peers)
            .unwrap()
            .unwrap()
            .1
    );
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 1);
}
