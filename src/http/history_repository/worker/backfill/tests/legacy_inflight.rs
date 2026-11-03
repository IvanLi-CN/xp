use super::*;
use sha2::Sha256;

// Keep the receiver absent in corruption cases so deletion-ledger freshness cannot mask
// signature, identity, chain or permanent-gap validation. The us4 regression below covers it.
#[tokio::test]
async fn worker_rejects_corrupt_legacy_pages_without_mutating_durable_state() {
    const NOW: u64 = 1_800_000_000;
    for (fault, expected_error) in [
        ("signature", "InvalidSignature"),
        (
            "identity",
            "legacy original journal identity or digest mismatch",
        ),
        ("chain", "legacy original page is not a continuous chain"),
        (
            "gap",
            "legacy original recovery overlaps a permanent source gap",
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let state = fixture::app_state(directory.path());
        let storage = HistoryStorage::open(directory.path());
        let identity = derived_repository_identity(
            &state,
            RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap(),
        )
        .unwrap();
        let key = derived_repository_signing_key(&state, &state.cluster.node_id).unwrap();
        for sequence in 0..2 {
            state
                .repository_replica
                .lock()
                .await
                .queue_local_source_segment(
                    &state.cluster.cluster_id,
                    identity.clone(),
                    &key,
                    vec![SyncRecord::new(
                        &state.cluster.node_id,
                        &state.cluster.node_id,
                        "traffic.v1",
                        1,
                        format!("corruption-{sequence}").into_bytes(),
                        b"retained".to_vec(),
                        false,
                    )],
                    NOW,
                )
                .unwrap()
                .unwrap();
        }
        let read_checkpoint = || {
            storage
                .read(crate::state::history_storage::REPOSITORY_REPLICA_KEY)
                .unwrap()
                .unwrap()
        };
        let mut snapshot: serde_json::Value = serde_json::from_slice(&read_checkpoint()).unwrap();
        assert!(snapshot["receiver"].is_null());
        let mut rows = storage.source_delivery_journal().unwrap();
        assert_eq!(rows.len(), 2);
        let original = SignedSegment::from_wire(&rows[1].wire).unwrap();
        original.verify_identity(&identity).unwrap();
        let first = original.canonical().first_cursor();
        assert_eq!(first.sequence(), 1);
        assert_eq!(
            original.canonical().previous_segment_hash(),
            Some(
                SignedSegment::from_wire(&rows[0].wire)
                    .unwrap()
                    .segment_hash()
                    .unwrap()
            )
        );
        match fault {
            "signature" => {
                // Retain a parseable, canonical wire and matching digest; fail cryptographic
                // verification rather than an unrelated wire/index integrity check.
                rows[1].wire = original
                    .canonical()
                    .sign(&ed25519_dalek::SigningKey::from_bytes(&[71; 32]))
                    .unwrap()
                    .wire_bytes()
                    .unwrap();
                assert!(
                    SignedSegment::from_wire(&rows[1].wire)
                        .unwrap()
                        .verify_identity(&identity)
                        .is_err()
                );
            }
            "identity" => {
                rows[1].identity = derived_repository_identity(
                    &state,
                    RepositoryNodeId::try_from("other-source".to_owned()).unwrap(),
                )
                .unwrap();
                assert_ne!(rows[1].identity, identity);
            }
            "chain" => {
                let broken = CanonicalSegment::new(
                    &state.cluster.cluster_id,
                    first.clone(),
                    original.canonical().records().to_vec(),
                    Some([42; 32]),
                    NOW,
                    NOW,
                )
                .unwrap()
                .sign(&key)
                .unwrap();
                broken.verify_identity(&identity).unwrap();
                rows[1].wire = broken.wire_bytes().unwrap();
                snapshot["local_source"]["streams"]["traffic"]["previous_segment_hash"] =
                    serde_json::json!(broken.segment_hash().unwrap());
            }
            "gap" => {
                snapshot["gaps"] = serde_json::json!([{
                    "source_node_id": state.cluster.node_id,
                    "source_epoch": first.source_epoch(), "stream": "traffic",
                    "first_sequence": 1, "last_sequence": 1,
                    "start_unix_seconds": NOW, "end_unix_seconds": NOW,
                    "permanent": true, "reason": "source_retention_expired",
                }]);
            }
            _ => unreachable!(),
        }
        let old_id = rows[1].id.clone();
        rows[1].id = hex::encode(Sha256::digest(&rows[1].wire));
        // Stage corruption only at the durable predecessor boundary. The actual worker must
        // reject it before converting or applying any part of this two-segment page.
        let connection =
            rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
        assert_eq!(connection.execute(
            "UPDATE source_delivery_journal SET id = ?1, identity = ?2, wire = ?3 WHERE id = ?4",
            rusqlite::params![
                rows[1].id, serde_json::to_vec(&rows[1].identity).unwrap(), rows[1].wire, old_id,
            ],
        ).unwrap(), 1);
        drop(connection);
        snapshot["local_history_backfill_inflight"] = serde_json::json!({
            "page_cursor": null, "completed": true,
            "segment_ids": rows.iter().map(|row| &row.id).collect::<Vec<_>>(),
        });
        storage
            .write(
                crate::state::history_storage::REPOSITORY_REPLICA_KEY,
                &serde_json::to_vec(&snapshot).unwrap(),
            )
            .unwrap();
        let checkpoint_before = read_checkpoint();
        let journal_before =
            serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap();
        drop(state);
        for attempt in 0..2 {
            let state = fixture::app_state(directory.path());
            let error = backfill_initial_repository_from_local_history(&state, NOW)
                .await
                .expect_err("corrupt predecessor page must fail closed")
                .to_string();
            assert!(
                error.contains(expected_error),
                "{fault}, restart {attempt}: {error}"
            );
            assert_eq!(
                read_checkpoint(),
                checkpoint_before,
                "{fault}: durable checkpoint"
            );
            assert_eq!(
                serde_json::to_value(storage.source_delivery_journal().unwrap()).unwrap(),
                journal_before,
                "{fault}: durable journal",
            );
            let runtime = state.repository_replica.lock().await;
            assert_eq!(runtime.local_source_next_sequence("traffic"), Some(2));
            assert_eq!(
                runtime.local_history_backfill_inflight_checkpoint(),
                Some((None, true))
            );
            assert!(!runtime.local_history_backfill_completed());
            let replay = runtime.local_history_backfill_inflight_segments().unwrap();
            assert_eq!(replay.len(), 2);
            for (replayed, row) in replay.iter().zip(&rows) {
                assert_eq!(replayed.wire, row.wire, "{fault}: pending wire");
                assert_eq!(replayed.identity, row.identity, "{fault}: pending identity");
            }
            assert_eq!(storage.repository_history_record_count().unwrap(), 0);
        }
        eprintln!("validated {fault}: {expected_error}; unchanged across two restarts");
    }
}

// Recover the real signed predecessor chain from a registered public HTTPS peer before ACK.
#[tokio::test]
async fn worker_recovers_us4_legacy_page_at_2262_and_continues_independent_history() {
    crate::http::history_repository::worker::backfill::legacy_recovery::network_tests::
        assert_us4_recovery_and_next_page().await;
}
