use ed25519_dalek::SigningKey;

use super::{RepositoryReplicaRuntime, StoredSegment};
use crate::{
    history_sync::{CanonicalSegment, Cursor, SignedSegment, SyncRecord},
    state::history_repository::{
        HistoryStorage,
        identity::{Ed25519PublicKey, RepositoryNodeId, RepositoryNodeIdentity, X25519PublicKey},
    },
    state::history_storage::Backend,
};

fn identity(signing_key: &SigningKey) -> RepositoryNodeIdentity {
    RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from("node-a".to_owned()).expect("node id"),
        Ed25519PublicKey::from_bytes(signing_key.verifying_key().to_bytes()).expect("signing key"),
        X25519PublicKey::from_bytes([12; 32]).expect("relay key"),
    )
    .expect("identity")
}

#[test]
fn sqlite_summary_keeps_tombstones_first_across_keyset_pages() {
    let temporary = tempfile::tempdir().expect("SQLite temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let repository_identity = identity(&signing_key);
    let signed = |stream: &str, tombstone: bool| {
        CanonicalSegment::new(
            "cluster-a",
            Cursor::new("node-a", 1, stream, 0).expect("cursor"),
            vec![SyncRecord::new(
                "subject-a",
                "node-a",
                "runtime.v1",
                1,
                stream.as_bytes().to_vec(),
                b"payload".to_vec(),
                tombstone,
            )],
            None,
            10,
            11,
        )
        .expect("segment")
        .sign(&signing_key)
        .expect("signed segment")
        .wire_bytes()
        .expect("wire")
    };
    let ordinary = StoredSegment {
        id: "000-ordinary".to_owned(),
        closed_at_unix_seconds: 10,
        identity: repository_identity.clone(),
        wire: signed("ordinary", false),
    };
    let tombstone_wire = signed("tombstone", true);
    let mut segments = vec![ordinary];
    segments.extend((0..256).map(|index| StoredSegment {
        id: format!("tombstone-{index:03}"),
        closed_at_unix_seconds: 11,
        identity: repository_identity.clone(),
        wire: tombstone_wire.clone(),
    }));
    let rows = segments
        .iter()
        .map(StoredSegment::sqlite_row)
        .collect::<Result<Vec<_>, _>>()
        .expect("SQLite rows");
    let runtime =
        RepositoryReplicaRuntime::load(HistoryStorage::open(temporary.path())).expect("runtime");
    runtime
        .storage
        .upsert_repository_history_segments(&rows)
        .expect("store interleaved segment IDs");

    let first = runtime.replication_summary().expect("first summary page");
    assert_eq!(first.segment_ids.len(), 256);
    assert!(
        first
            .segment_ids
            .iter()
            .all(|id| id.starts_with("tombstone-"))
    );
    let cursor = first.next_segment_id.expect("tombstone page cursor");
    assert!(cursor.starts_with("t:"));

    let second = runtime
        .replication_summary_after(Some(&cursor), false)
        .expect("ordinary summary page");
    assert_eq!(second.segment_ids, ["000-ordinary"]);
    assert!(second.next_segment_id.is_none());
}

#[test]
fn sqlite_summary_orders_repair_candidates_by_signed_cursor() {
    let temporary = tempfile::tempdir().expect("SQLite temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let repository_identity = identity(&signing_key);
    let ready = vec!["repository-a".to_owned(), "repository-b".to_owned()];
    let mut primary =
        RepositoryReplicaRuntime::load(HistoryStorage::open(temporary.path())).expect("runtime");
    let mut previous_hash = None;

    for sequence in 0_u64..66 {
        let signed = CanonicalSegment::new(
            "cluster-a",
            Cursor::new("node-a", 7, "runtime", sequence).expect("cursor"),
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                format!("runtime:{sequence}").into_bytes(),
                format!("sample:{sequence}").into_bytes(),
                false,
            )],
            previous_hash,
            100 + sequence,
            100 + sequence,
        )
        .expect("canonical segment")
        .sign(&signing_key)
        .expect("signed segment");
        previous_hash = Some(signed.segment_hash().expect("segment hash"));
        primary
            .receive_wire_from_repository(
                "cluster-a",
                &repository_identity,
                &signed.wire_bytes().expect("wire"),
                100 + sequence,
                &ready,
                "repository-a",
            )
            .expect("primary accepts ordered source segment");
    }

    let summary = primary.replication_summary().expect("SQLite summary");
    let missing = summary.segment_ids[..64].to_vec();
    let repair = primary.repair_batch(&missing).expect("repair batch");
    let sequences = repair
        .segments
        .iter()
        .map(|segment| {
            SignedSegment::from_wire(&segment.wire)
                .expect("repair wire")
                .canonical()
                .first_cursor()
                .sequence()
        })
        .collect::<Vec<_>>();
    assert_eq!(sequences, (0_u64..64).collect::<Vec<_>>());

    let standby_temporary = tempfile::tempdir().expect("standby SQLite temporary directory");
    let mut standby =
        RepositoryReplicaRuntime::load(HistoryStorage::open(standby_temporary.path()))
            .expect("standby runtime");
    for segment in repair.segments {
        standby
            .receive_wire_from_repository(
                "cluster-a",
                &segment.identity,
                &segment.wire,
                200,
                &ready,
                "repository-b",
            )
            .expect("SQLite repair segments must be accepted in source cursor order");
    }
}

#[test]
fn sqlite_load_defers_legacy_segment_cursor_index_migration() {
    let temporary = tempfile::tempdir().expect("SQLite temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let repository_identity = identity(&signing_key);
    let ready = vec!["repository-a".to_owned()];
    let mut runtime =
        RepositoryReplicaRuntime::load(HistoryStorage::open(temporary.path())).expect("runtime");
    let mut previous_hash = None;
    for sequence in 0_u64..2 {
        let signed = CanonicalSegment::new(
            "cluster-a",
            Cursor::new("node-a", 7, "runtime", sequence).expect("cursor"),
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                format!("runtime:{sequence}").into_bytes(),
                format!("sample:{sequence}").into_bytes(),
                false,
            )],
            previous_hash,
            100,
            100,
        )
        .expect("canonical segment")
        .sign(&signing_key)
        .expect("signed segment");
        previous_hash = Some(signed.segment_hash().expect("segment hash"));
        runtime
            .receive_wire_from_repository(
                "cluster-a",
                &repository_identity,
                &signed.wire_bytes().expect("wire"),
                100,
                &ready,
                "repository-a",
            )
            .expect("store segment");
    }
    {
        let mut backend = runtime.storage.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            panic!("test requires SQLite storage");
        };
        connection
            .execute(
                "UPDATE repository_history_segments
                 SET source_node_id = '', source_epoch = 0, stream = '', first_sequence = 0",
                [],
            )
            .expect("erase legacy cursor index");
    }
    drop(runtime);

    let mut restored =
        RepositoryReplicaRuntime::load(HistoryStorage::open(temporary.path())).expect("restore");
    assert_eq!(
        restored
            .storage
            .repository_history_segments_missing_cursor_index(None, 3)
            .expect("inspect cursor index")
            .len(),
        2
    );
    assert!(
        !restored
            .migrate_legacy_segment_cursor_index_page(1)
            .expect("migrate one bounded page")
    );
    drop(restored);

    let mut restored =
        RepositoryReplicaRuntime::load(HistoryStorage::open(temporary.path())).expect("resume");
    assert_eq!(
        restored
            .storage
            .repository_history_segments_missing_cursor_index(None, 3)
            .expect("inspect resumed cursor index")
            .len(),
        1
    );
    assert!(
        !restored
            .migrate_legacy_segment_cursor_index_page(1)
            .expect("migrate resumed bounded page")
    );
    assert!(
        restored
            .storage
            .repository_history_segments_missing_cursor_index(None, 3)
            .expect("inspect cursor index")
            .is_empty()
    );
    assert!(
        restored
            .migrate_legacy_segment_cursor_index_page(1)
            .expect("complete migration")
    );
    let summary = restored.replication_summary().expect("SQLite summary");
    let repair = restored
        .repair_batch(&summary.segment_ids)
        .expect("repair batch");
    let sequences = repair
        .segments
        .iter()
        .map(|segment| {
            SignedSegment::from_wire(&segment.wire)
                .expect("repair wire")
                .canonical()
                .first_cursor()
                .sequence()
        })
        .collect::<Vec<_>>();
    assert_eq!(sequences, vec![0, 1]);
}
