use super::*;

#[test]
fn final_tiered_page_keeps_the_signed_repair_anchor_until_lease_expiry() {
    let temporary = tempfile::tempdir().unwrap();
    let mut runtime = load(temporary.path());
    let key = signing_key();
    let identity = identity(&key);
    let first = segment_at(&key, 0, vec![record(b"first", false)], None, 20_000, 20_001);
    let second = segment_at(
        &key,
        1,
        vec![record(b"second", false)],
        Some(first.segment_hash().unwrap()),
        20_000,
        20_001,
    );
    for signed in [&first, &second] {
        runtime
            .receive_wire(
                "cluster-a",
                &identity,
                &signed.wire_bytes().unwrap(),
                20_002,
            )
            .unwrap();
    }
    let ids = runtime.replication_summary().unwrap().segment_ids;
    assert_eq!(ids.len(), 2);
    let now = 20_002
        + super::super::super::RepositoryRetentionPolicy::default().minute_retention_seconds();
    let cutoff =
        now - super::super::super::RepositoryRetentionPolicy::default().minute_retention_seconds();
    let page = runtime.tiered_backfill_page(None, 1, cutoff, now).unwrap();
    assert!(page.next_cursor.is_some());
    let last = runtime
        .tiered_backfill_page(page.next_cursor.as_deref(), 1, cutoff, now)
        .unwrap();
    assert!(last.next_cursor.is_none());
    drop(runtime);
    let mut runtime = load(temporary.path());
    runtime.prepare_for_replication(now + 15 * 60 - 1).unwrap();
    let repair = runtime.repair_batch(&ids).unwrap();
    assert_eq!(repair.segments.len(), 2);
    assert!(repair.unavailable_segment_ids.is_empty());
    runtime.prepare_for_replication(now + 15 * 60).unwrap();
    let repair = runtime.repair_batch(&ids).unwrap();
    assert!(repair.segments.is_empty());
    assert_eq!(repair.unavailable_segment_ids.len(), 2);
}
