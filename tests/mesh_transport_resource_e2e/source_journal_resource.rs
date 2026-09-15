use super::*;

pub async fn run_source_delivery_journal_resource_workload(binary: &Path) -> u64 {
    let temp = tempfile::tempdir().expect("source journal resource data directory");
    let bind_port = reserve_local_port();
    run_init(binary, temp.path(), bind_port);
    prepare_source_delivery_storage(temp.path());
    let log_path = temp.path().join("source-journal.log");
    let mut child = spawn_xp(binary, temp.path(), bind_port, "source-journal");
    wait_for_xp(&mut child, bind_port, &log_path).await;
    let pid = child.id();
    assert_expected_memory_scope(pid);

    let mut peak_pss_kib = read_pss(pid).expect("read source journal XP PSS").total_kib;
    for _ in 0..10 {
        peak_pss_kib =
            peak_pss_kib.max(read_pss(pid).expect("read source journal XP PSS").total_kib);
        sleep(Duration::from_millis(100)).await;
    }
    println!("source_journal_resource_candidate_peak_pss_kib={peak_pss_kib}");
    stop_child(&mut child).await;
    peak_pss_kib
}

fn prepare_source_delivery_storage(data_dir: &Path) {
    let connection = rusqlite::Connection::open(data_dir.join("history.sqlite3"))
        .expect("open source journal resource database");
    connection
        .pragma_update(None, "cache_size", -1024_i64)
        .expect("limit source journal SQLite cache");
    connection
        .pragma_update(None, "wal_autocheckpoint", 100_i64)
        .expect("limit source journal WAL checkpoint");
    let identity = serde_json::to_vec(&serde_json::json!({
        "node_id": "source-node",
        "ed25519_public_key": URL_SAFE_NO_PAD.encode([7_u8; 32]),
        "x25519_relay_public_key": URL_SAFE_NO_PAD.encode([8_u8; 32]),
    }))
    .expect("encode source journal identity");
    let wire = vec![0_u8; 27 * 1024];
    for batch_start in (0..20_000_i64).step_by(256) {
        let transaction = connection
            .unchecked_transaction()
            .expect("begin source journal backlog transaction");
        for sequence in batch_start..(batch_start + 256).min(20_000) {
            transaction
                .execute(
                    "INSERT INTO source_delivery_journal
                         (id, stream, closed_at, identity, wire, created_at,
                          source_node_id, source_epoch, first_sequence)
                     VALUES (?1, 'runtime', 100, ?2, ?3, 100, 'source-node', 1, ?4)",
                    rusqlite::params![format!("{sequence:064x}"), &identity, &wire, sequence,],
                )
                .expect("insert source journal resource row");
        }
        transaction
            .commit()
            .expect("commit source journal backlog transaction");
    }
    connection
        .execute(
            "UPDATE source_delivery_journal_state
             SET pending_segments = 20_000,
                 pending_bytes = ?1,
                 epoch_high_water = 1,
                 order_repair_completed = 1,
                 capacity_suspended = 1,
                 stream_counts_initialized = 1
             WHERE singleton = 1",
            [i64::from(wire.len() as u32) * 20_000],
        )
        .expect("record source journal resource backlog");
    connection
        .execute(
            "INSERT OR REPLACE INTO source_delivery_journal_stream_state
             (stream, pending_segments) VALUES ('runtime', 20_000)",
            [],
        )
        .expect("record source journal stream backlog");
}
