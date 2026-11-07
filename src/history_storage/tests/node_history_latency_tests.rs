use super::*;

/// Run explicitly on the same Linux libc/toolchain as the deployable candidate.
#[test]
#[ignore = "candidate libc latency regression; run in resource-evidence"]
fn large_pretty_node_history_guard_stays_bounded() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    assert!(storage.is_sqlite());
    let records = vec![serde_json::json!({"observed_at": 123, "value": 456}); 16_384];
    let payload = serde_json::to_vec_pretty(&serde_json::json!({
        "deletion_version": 1,
        "retained": records,
    }))
    .unwrap();
    assert!(payload.len() > 800_000);
    let digest = [7; 32];
    storage
        .write_node_history_guarded(&payload, 1, &digest)
        .unwrap();
    let started = std::time::Instant::now();
    storage.check_node_history_guard(1, &digest).unwrap();
    let elapsed = started.elapsed();
    println!(
        "node_history_guard payload_bytes={} elapsed_ms={}",
        payload.len(),
        elapsed.as_millis()
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "SQLite JSON guard blocks execution: {elapsed:?}"
    );
    assert!(storage.check_node_history_guard(0, &digest).is_err());
    assert!(storage.check_node_history_guard(1, &[8; 32]).is_err());
}
