use super::tests::load;

#[test]
fn dynamic_relay_hourly_attempt_gate_survives_repository_restart() {
    let first = tempfile::tempdir().expect("first repository");
    let mut sender = load(first.path());
    sender.snapshot.cluster_id = Some("cluster-a".to_owned());
    sender.snapshot.legacy_segment_cursor_index_complete = false;
    assert!(
        !sender
            .begin_dynamic_relay_attempt(10_000)
            .expect("pending legacy index skips relay attempt")
    );
    assert_eq!(
        sender.snapshot.last_dynamic_relay_attempt_unix_seconds,
        None
    );
    sender.snapshot.legacy_segment_cursor_index_complete = true;
    assert!(
        sender
            .begin_dynamic_relay_attempt(10_000)
            .expect("first relay attempt is due")
    );
    assert!(
        !sender
            .begin_dynamic_relay_attempt(10_001)
            .expect("relay attempt is rate limited")
    );
    let mut restored = load(first.path());
    assert!(
        !restored
            .begin_dynamic_relay_attempt(10_001)
            .expect("relay attempt stays rate limited after restart")
    );
}
