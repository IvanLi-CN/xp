use ed25519_dalek::SigningKey;
use sha2::Digest as _;

use super::{
    RepositoryRepairBatch, RepositoryReplicaRuntime, RepositoryRuntimeError, StoredSegment,
};
use crate::{
    history_sync::{
        CanonicalSegment, Cursor, DirectPath, MAX_RELAY_PLAINTEXT_BYTES, SignedSegment, SyncRecord,
    },
    state::history_repository::{
        HistoryStorage,
        identity::{Ed25519PublicKey, RepositoryNodeId, RepositoryNodeIdentity, X25519PublicKey},
        replica::{ReplicaError, ReplicaWork, RepositoryReplicaGap},
    },
};

fn identity() -> RepositoryNodeIdentity {
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from("node-a".to_owned()).expect("node id"),
        Ed25519PublicKey::from_bytes(signing_key.verifying_key().to_bytes()).expect("signing key"),
        X25519PublicKey::from_bytes([12; 32]).expect("relay key"),
    )
    .expect("identity")
}

fn load(path: &std::path::Path) -> RepositoryReplicaRuntime {
    RepositoryReplicaRuntime::load(HistoryStorage::open(path)).expect("runtime")
}

#[test]
fn relay_repair_pages_through_the_full_bounded_segment_history_after_restart() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let identity = identity();
    let mut runtime = load(temporary.path());
    runtime.snapshot.external_history = false;
    runtime.snapshot.segments = (0_u8..128)
        .map(|sequence| StoredSegment {
            id: format!("segment-{sequence:03}"),
            closed_at_unix_seconds: u64::from(sequence),
            identity: identity.clone(),
            wire: vec![sequence],
        })
        .collect();

    let first = runtime
        .relay_batch("repository-b")
        .expect("first bounded relay page");
    assert_eq!(first.batch.segments.len(), 64);
    assert_eq!(first.batch.segments[0].wire, vec![0]);
    assert_eq!(first.batch.segments[63].wire, vec![63]);
    let next_segment_id = first
        .next_segment_id()
        .expect("next retained segment")
        .to_owned();
    runtime.snapshot.segments.drain(..64);
    runtime
        .snapshot
        .segments
        .extend((128_u8..192).map(|sequence| StoredSegment {
            id: format!("segment-{sequence:03}"),
            closed_at_unix_seconds: u64::from(sequence),
            identity: identity.clone(),
            wire: vec![sequence],
        }));
    runtime
        .record_relay_batch_delivered("repository-b", Some(&next_segment_id))
        .expect("persist relay cursor");

    let restored = load(temporary.path());
    let second = restored
        .relay_batch("repository-b")
        .expect("second bounded relay page");
    assert_eq!(second.batch.segments.len(), 64);
    assert_eq!(second.batch.segments[0].wire, vec![64]);
    assert_eq!(second.batch.segments[63].wire, vec![127]);
}

#[test]
fn relay_repair_pages_by_frame_budget_and_advances_through_large_backlog() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let identity = identity();
    let mut runtime = load(temporary.path());
    runtime.snapshot.external_history = false;
    runtime.snapshot.segments = (0_u8..4)
        .map(|sequence| StoredSegment {
            id: format!("large-segment-{sequence}"),
            closed_at_unix_seconds: u64::from(sequence),
            identity: identity.clone(),
            wire: {
                let mut state = u32::from(sequence).saturating_add(1);
                (0..110 * 1024)
                    .map(|_| {
                        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        state.to_be_bytes()[0]
                    })
                    .collect()
            },
        })
        .collect();

    let first = runtime
        .relay_batch("repository-b")
        .expect("first frame-sized relay page");
    assert!(!first.batch.segments.is_empty());
    assert!(first.batch.segments.len() < 4);
    assert!(first.payload.len() <= MAX_RELAY_PLAINTEXT_BYTES);
    let first_count = first.batch.segments.len();
    let next_segment_id = first
        .next_segment_id()
        .expect("next retained segment")
        .to_owned();
    runtime
        .record_relay_batch_delivered("repository-b", Some(&next_segment_id))
        .expect("persist first relay page cursor");

    let mut delivered = first_count;
    while delivered < 4 {
        let page = runtime
            .relay_batch("repository-b")
            .expect("next frame-sized relay page");
        assert!(!page.batch.segments.is_empty());
        assert_eq!(
            page.batch.segments[0].wire,
            runtime.snapshot.segments[delivered].wire
        );
        assert!(page.payload.len() <= MAX_RELAY_PLAINTEXT_BYTES);
        delivered += page.batch.segments.len();
        let next_segment_id = page
            .next_segment_id()
            .expect("next retained segment")
            .to_owned();
        runtime
            .record_relay_batch_delivered("repository-b", Some(&next_segment_id))
            .expect("persist relay page cursor");
    }
    assert_eq!(delivered, 4);
}

#[test]
fn relay_payload_rejects_malformed_gap_ranges_before_delivery() {
    let invalid_batch = RepositoryRepairBatch {
        segments: Vec::new(),
        gaps: vec![RepositoryReplicaGap {
            source_node_id: "node-a".to_owned(),
            source_epoch: 7,
            stream: "runtime".to_owned(),
            first_sequence: 2,
            last_sequence: 1,
            start_unix_seconds: 101,
            end_unix_seconds: 100,
            permanent: true,
        }],
    };
    let serialized = serde_json::to_vec(&invalid_batch).expect("relay batch JSON");
    let payload = zstd::stream::encode_all(std::io::Cursor::new(serialized), 1)
        .expect("compressed relay batch");

    assert!(matches!(
        RepositoryRepairBatch::from_relay_payload(&payload),
        Err(RepositoryRuntimeError::Replica(ReplicaError::InvalidRange))
    ));
}

#[test]
fn repair_batch_accepts_a_pre_gap_metadata_payload() {
    let batch = serde_json::from_slice::<RepositoryRepairBatch>(br#"{"segments":[]}"#)
        .expect("legacy repair payload remains readable");
    assert!(batch.segments.is_empty());
    assert!(batch.gaps.is_empty());
}

#[test]
fn repair_batch_orders_same_stream_segments_by_cursor_not_segment_id() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut primary = load(temporary.path());
    let mut previous_hash = None;
    let mut segment_ids = Vec::new();

    for sequence in 0_u64..10 {
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
        let wire = signed.wire_bytes().expect("wire bytes");
        primary
            .receive_wire("cluster-a", &source_identity, &wire, 100 + sequence)
            .expect("primary accepts ordered source segment");
        previous_hash = Some(signed.segment_hash().expect("segment hash"));
        segment_ids.push(hex::encode(sha2::Sha256::digest(&wire)));
    }

    let repair = primary.repair_batch(&segment_ids).expect("repair batch");
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
    assert_eq!(sequences, (0_u64..10).collect::<Vec<_>>());
}

#[test]
fn local_source_outbox_survives_restart_until_the_primary_acknowledges_it() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let first = load(temporary.path())
        .queue_local_source_segment(
            "cluster-a",
            source_identity.clone(),
            &signing_key,
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                b"runtime:100".to_vec(),
                b"sample".to_vec(),
                false,
            )],
            100,
        )
        .expect("queue source segment")
        .expect("source has a record");

    let mut restored = load(temporary.path());
    let retry = restored
        .queue_local_source_segment(
            "cluster-a",
            source_identity.clone(),
            &signing_key,
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                b"runtime:101".to_vec(),
                b"new sample".to_vec(),
                false,
            )],
            101,
        )
        .expect("reuse queued source segment")
        .expect("pending source segment");
    assert_eq!(retry.wire, first.wire);

    let retry_after_restart = restored
        .queue_local_source_segment(
            "cluster-a",
            source_identity.clone(),
            &signing_key,
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                b"runtime:102".to_vec(),
                b"third sample".to_vec(),
                false,
            )],
            102,
        )
        .expect("queue third source sample")
        .expect("first source segment remains pending");
    assert_eq!(retry_after_restart.wire, first.wire);

    restored
        .acknowledge_local_source_segment(&retry.wire)
        .expect("acknowledge source segment");
    let next = restored
        .queue_local_source_segment(
            "cluster-a",
            source_identity.clone(),
            &signing_key,
            Vec::new(),
            103,
        )
        .expect("read second pending source segment")
        .expect("second source segment");
    assert_eq!(source_record_key(&next), b"runtime:101");
    restored
        .acknowledge_local_source_segment(&next.wire)
        .expect("acknowledge second source segment");
    let third = restored
        .queue_local_source_segment("cluster-a", source_identity, &signing_key, Vec::new(), 103)
        .expect("read third pending source segment")
        .expect("third source segment");
    assert_eq!(source_record_key(&third), b"runtime:102");
}

#[test]
fn source_tombstone_acknowledgements_use_the_tombstone_cursor_stream() {
    let source_dir = tempfile::tempdir().expect("source directory");
    let collector_dir = tempfile::tempdir().expect("collector directory");
    let key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let ready = vec!["repo-a".to_owned(), "repo-b".to_owned()];
    let tombstone = SyncRecord::new(
        "node-a",
        "node-a",
        "traffic.v1",
        1,
        b"node-history:node:node-a:".to_vec(),
        b"deleted".to_vec(),
        true,
    );
    let mut source = load(source_dir.path());
    let queued = source
        .queue_local_source_segments_for_repositories(
            "cluster-a",
            source_identity.clone(),
            &key,
            vec![tombstone],
            100,
            &ready,
        )
        .expect("queue tombstone");
    let mut collector = load(collector_dir.path());
    let receipt = collector
        .receive_wire_from_repository(
            "cluster-a",
            &source_identity,
            &queued[0].wire,
            100,
            &ready,
            "repo-a",
        )
        .expect("collector accepts tombstone");
    let acknowledgement = receipt.tombstone_acknowledgements()[0].clone();
    source
        .acknowledge_tombstones(&[acknowledgement.clone()])
        .expect("source accepts collector acknowledgement");
    assert!(!source.tombstones.fully_acknowledged(&acknowledgement.key));

    let mut second = acknowledgement;
    second.repository_id = "repo-b".to_owned();
    source
        .acknowledge_tombstones(&[second.clone()])
        .expect("source accepts second acknowledgement");
    assert!(source.tombstones.fully_acknowledged(&second.key));
}

#[test]
fn source_epoch_rotates_when_the_replica_snapshot_is_lost_but_sqlite_remains() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let storage = HistoryStorage::open(temporary.path());
    let mut source = RepositoryReplicaRuntime::load(storage.clone()).expect("first runtime");
    let first = source
        .queue_local_source_segments(
            "cluster-a",
            source_identity.clone(),
            &key,
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                b"node-history:node:node-a:100".to_vec(),
                b"first".to_vec(),
                false,
            )],
            100,
        )
        .expect("first source segment");
    let first_epoch = SignedSegment::from_wire(&first[0].wire)
        .expect("first wire")
        .canonical()
        .first_cursor()
        .source_epoch();
    storage
        .write(crate::state::history_storage::REPOSITORY_REPLICA_KEY, b"{}")
        .expect("simulate replica snapshot loss");
    let mut restored = RepositoryReplicaRuntime::load(storage).expect("restored runtime");
    let next = restored
        .queue_local_source_segments(
            "cluster-a",
            source_identity,
            &key,
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                b"node-history:node:node-a:101".to_vec(),
                b"second".to_vec(),
                false,
            )],
            101,
        )
        .expect("rotated source segment");
    let next_epoch = SignedSegment::from_wire(&next[0].wire)
        .expect("next wire")
        .canonical()
        .first_cursor()
        .source_epoch();
    assert_eq!(next_epoch, first_epoch.saturating_add(1));
}

fn source_record_key(segment: &super::RepositoryReplicaSegment) -> Vec<u8> {
    SignedSegment::from_wire(&segment.wire)
        .expect("signed source segment")
        .canonical()
        .records()[0]
        .record_key()
        .to_vec()
}

#[test]
fn saturated_source_outbox_returns_front_and_drains_after_recovery() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    for sequence in 0..8_u64 {
        load(temporary.path())
            .queue_local_source_segment(
                "cluster-a",
                source_identity.clone(),
                &signing_key,
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    format!("runtime:{sequence}").into_bytes(),
                    b"sample".to_vec(),
                    false,
                )],
                sequence,
            )
            .expect("queue source segment")
            .expect("source has a record");
    }

    let mut runtime = load(temporary.path());
    let front = runtime
        .queue_local_source_segment(
            "cluster-a",
            source_identity.clone(),
            &signing_key,
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                b"runtime:8".to_vec(),
                b"backpressured".to_vec(),
                false,
            )],
            8,
        )
        .expect("return queued front while backpressured")
        .expect("front remains available");
    assert_eq!(source_record_key(&front), b"runtime:0");
    runtime
        .acknowledge_local_source_segment(&front.wire)
        .expect("acknowledge source front");
    let next = runtime
        .queue_local_source_segment("cluster-a", source_identity, &signing_key, Vec::new(), 9)
        .expect("drain queued source segment after recovery")
        .expect("next segment remains queued");
    assert_eq!(source_record_key(&next), b"runtime:1");
}

#[test]
fn a_backpressured_stream_preserves_other_streams_and_a_permanent_gap() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());
    for sequence in 0..8_u64 {
        runtime
            .queue_local_source_segment(
                "cluster-a",
                source_identity.clone(),
                &signing_key,
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    format!("runtime:{sequence}").into_bytes(),
                    b"sample".to_vec(),
                    false,
                )],
                sequence,
            )
            .expect("queue source segment");
    }
    let segments = runtime
        .queue_local_source_segments(
            "cluster-a",
            source_identity,
            &signing_key,
            vec![
                SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    b"runtime:8".to_vec(),
                    b"dropped".to_vec(),
                    false,
                ),
                SyncRecord::new(
                    "node-a",
                    "node-a",
                    "traffic.v1",
                    1,
                    b"traffic:8".to_vec(),
                    b"queued".to_vec(),
                    false,
                ),
            ],
            8,
        )
        .expect("queue independent source streams");
    let streams = segments
        .iter()
        .map(|segment| {
            SignedSegment::from_wire(&segment.wire)
                .expect("signed segment")
                .canonical()
                .first_cursor()
                .stream()
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert!(streams.contains(&"runtime".to_owned()));
    assert!(streams.contains(&"traffic".to_owned()));
    let gaps = runtime.local_source_backpressure_gaps("node-a");
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0].stream, "runtime");
    assert_eq!(gaps[0].first_sequence, 8);
    assert_eq!(gaps[0].last_sequence, 8);
    assert!(gaps[0].permanent);
    assert_eq!(gaps[0].start_unix_seconds, 8);
    assert_eq!(gaps[0].end_unix_seconds, 8);
    assert_eq!(
        runtime.snapshot.local_source.streams["runtime"].next_sequence,
        9
    );
}

#[test]
fn local_source_segments_keep_each_observation_stream_independent() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let segments = load(temporary.path())
        .queue_local_source_segments(
            "cluster-a",
            identity(),
            &signing_key,
            vec![
                SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    b"runtime:100".to_vec(),
                    b"runtime".to_vec(),
                    false,
                ),
                SyncRecord::new(
                    "node-a",
                    "node-a",
                    "traffic.v1",
                    1,
                    b"traffic:100".to_vec(),
                    b"traffic".to_vec(),
                    false,
                ),
            ],
            100,
        )
        .expect("queue independent source streams");
    assert_eq!(segments.len(), 2);
    let mut streams = segments
        .iter()
        .map(|segment| {
            SignedSegment::from_wire(&segment.wire)
                .expect("signed segment")
                .canonical()
                .first_cursor()
                .stream()
                .to_owned()
        })
        .collect::<Vec<_>>();
    streams.sort_unstable();
    assert_eq!(streams, ["runtime", "traffic"]);
}

#[test]
fn direct_path_selection_retries_a_failed_tunnel_after_its_probe_window() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut runtime = load(temporary.path());
    assert_eq!(
        runtime
            .select_peer_direct_path("peer-a", false, 100)
            .expect("select tunnel without Mesh")
            .0,
        DirectPath::CloudflareTunnel
    );
    runtime
        .record_peer_direct_path_result("peer-a", DirectPath::CloudflareTunnel, false, 100)
        .expect("record failed tunnel");
    assert!(
        runtime
            .select_peer_direct_path("peer-a", false, 101)
            .is_err()
    );
    assert_eq!(
        runtime
            .select_peer_direct_path("peer-a", false, 400)
            .expect("retry tunnel probe")
            .0,
        DirectPath::CloudflareTunnel
    );
}

#[test]
fn direct_path_selection_keeps_a_continuously_healthy_path_stable() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut runtime = load(temporary.path());
    assert_eq!(
        runtime
            .select_peer_direct_path("peer-a", true, 100)
            .expect("select initial Mesh path")
            .0,
        DirectPath::RealityMesh
    );
    runtime
        .record_peer_direct_path_result("peer-a", DirectPath::RealityMesh, true, 100)
        .expect("record Mesh success");
    runtime
        .record_peer_direct_path_result("peer-a", DirectPath::CloudflareTunnel, true, 200)
        .expect("record tunnel probe success");
    runtime
        .record_peer_direct_path_result("peer-a", DirectPath::RealityMesh, true, 400)
        .expect("record continued Mesh success");
    assert_eq!(
        runtime
            .select_peer_direct_path("peer-a", true, 800)
            .expect("keep stable Mesh path")
            .0,
        DirectPath::RealityMesh
    );
}

#[test]
fn source_switches_to_its_standby_after_three_primary_delivery_failures() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut runtime = load(temporary.path());
    for _ in 0..3 {
        runtime
            .record_local_source_collector_delivery("repository-a", "repository-a", false)
            .expect("record failed primary delivery");
    }
    assert_eq!(
        runtime.local_source_collector("repository-a", Some("repository-b")),
        "repository-b"
    );
    let restored = load(temporary.path());
    assert_eq!(
        restored.local_source_collector("repository-a", Some("repository-b")),
        "repository-b"
    );
}

#[test]
fn source_failure_state_resets_when_rendezvous_primary_changes() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut runtime = load(temporary.path());
    for _ in 0..3 {
        runtime
            .record_local_source_collector_delivery("repository-a", "repository-a", false)
            .expect("record failed primary delivery");
    }
    assert_eq!(
        runtime.local_source_collector("repository-a", Some("repository-b")),
        "repository-b"
    );
    runtime
        .record_local_source_collector_delivery("repository-c", "repository-c", false)
        .expect("record new primary failure");
    assert_eq!(
        runtime.local_source_collector("repository-c", Some("repository-b")),
        "repository-c"
    );
}

#[test]
fn active_tombstone_acknowledgements_retry_after_restart_until_delivered() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let tombstone = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "runtime", 0).expect("cursor"),
        vec![SyncRecord::new(
            "subject-a",
            "node-a",
            "runtime.v1",
            1,
            b"dead".to_vec(),
            b"sample".to_vec(),
            true,
        )],
        None,
        10,
        11,
    )
    .expect("segment")
    .sign(&signing_key)
    .expect("signature");
    let ready = vec!["repo-a".to_owned(), "repo-b".to_owned()];
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire_from_repository(
            "cluster-a",
            &identity(),
            &tombstone.wire_bytes().expect("wire"),
            100,
            &ready,
            "repo-a",
        )
        .expect("tombstone");

    let failed_delivery = runtime
        .tombstone_acknowledgement_page("repo-a")
        .expect("pending acknowledgement");
    assert_eq!(failed_delivery.acknowledgements().len(), 1);
    let pending_key = failed_delivery.acknowledgements()[0].key.clone();

    let mut restored = load(temporary.path());
    let retry = restored
        .tombstone_acknowledgement_page("repo-a")
        .expect("restart preserves pending acknowledgement");
    assert_eq!(retry.acknowledgements().len(), 1);
    assert_eq!(retry.acknowledgements()[0].key, pending_key);

    restored
        .record_tombstone_acknowledgement_delivery(retry.next_cursor())
        .expect("record complete fanout");
    let replay = load(temporary.path())
        .tombstone_acknowledgement_page("repo-a")
        .expect("active tombstones continue to cover new ready peers");
    assert_eq!(replay.acknowledgements().len(), 1);
}

#[test]
fn rendezvous_failover_tracks_an_unseen_membership_source() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let ready = vec![
        "repository-a".to_owned(),
        "repository-b".to_owned(),
        "repository-c".to_owned(),
    ];
    let source = "new-source";
    let assignment = super::super::rendezvous_collectors(source, &ready).expect("assignment");
    let standby = assignment.standby().expect("standby").to_owned();
    let mut runtime = load(temporary.path());

    for now in [300, 600, 900] {
        runtime
            .record_stale_collection_cycles(now, &ready, assignment.primary(), &[source.to_owned()])
            .expect("record unseen source failure");
    }

    assert!(
        runtime
            .collects_source(source, &ready, &standby)
            .expect("standby takes an unseen source after three cycles")
    );
}

#[test]
fn deep_verification_rotates_through_every_ready_peer_before_completing() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let ready = [
        "repository-local".to_owned(),
        "repository-a".to_owned(),
        "repository-b".to_owned(),
        "repository-c".to_owned(),
        "repository-d".to_owned(),
        "repository-e".to_owned(),
        "repository-f".to_owned(),
    ];
    let mut runtime = load(temporary.path());
    let first = runtime
        .next_replication_peers(&ready, "repository-local", 4)
        .expect("first peer page");
    assert_eq!(
        first,
        [
            "repository-a",
            "repository-b",
            "repository-c",
            "repository-d"
        ]
    );
    for peer in &first {
        assert!(
            !runtime
                .record_direct_peer_deep_verification(
                    peer,
                    &ready,
                    "repository-local",
                    ReplicaWork::DeepVerification,
                )
                .expect("partial deep verification")
        );
    }

    let mut restored = load(temporary.path());
    let second = restored
        .next_replication_peers(&ready, "repository-local", 4)
        .expect("second peer page");
    assert_eq!(
        second,
        [
            "repository-e",
            "repository-f",
            "repository-a",
            "repository-b"
        ]
    );
    let mut completed = false;
    for peer in &second {
        completed |= restored
            .record_direct_peer_deep_verification(
                peer,
                &ready,
                "repository-local",
                ReplicaWork::DeepVerification,
            )
            .expect("direct peer verification");
    }
    assert!(completed);
}
