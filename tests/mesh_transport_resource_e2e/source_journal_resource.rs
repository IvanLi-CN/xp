use super::*;

const JOURNAL_CPU_P95_LIMIT_PERCENT: f64 = 9.0;
const JOURNAL_READ_BYTES_LIMIT: u64 = 4 * 1024 * 1024;
const JOURNAL_RSS_DELTA_LIMIT: u64 = 2 * 1024 * 1024;

fn read_process_read_bytes(pid: u32) -> u64 {
    fs::read_to_string(format!("/proc/{pid}/io"))
        .expect("read XP process I/O")
        .lines()
        .find_map(|line| line.strip_prefix("read_bytes:")?.trim().parse().ok())
        .expect("read XP process read_bytes")
}

fn read_process_rss_bytes(pid: u32) -> u64 {
    let resident_pages = fs::read_to_string(format!("/proc/{pid}/statm"))
        .expect("read XP process statm")
        .split_whitespace()
        .nth(1)
        .expect("resident page count")
        .parse::<u64>()
        .expect("parse resident page count");
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    assert!(page_size > 0, "system page size must be positive");
    resident_pages.saturating_mul(page_size as u64)
}

pub(super) async fn stop_child(child: &mut XpProcess) {
    if let Some(unit) = child.unit.as_deref() {
        let _ = Command::new("systemctl")
            .args(["--user", "stop", unit])
            .status();
    } else {
        unsafe {
            libc::kill(child.child.id() as libc::pid_t, libc::SIGINT);
        }
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let child_stopped = child.child.try_wait().expect("poll stopped XP").is_some();
        let unit_active = child.unit.as_deref().is_some_and(|unit| {
            Command::new("systemctl")
                .args(["--user", "is-active", "--quiet", unit])
                .status()
                .is_ok_and(|status| status.success())
        });
        if child_stopped && !unit_active {
            return;
        }
        sleep(Duration::from_millis(50)).await;
    }
    if let Some(unit) = child.unit.as_deref() {
        let _ = Command::new("systemctl")
            .args(["--user", "kill", "--kill-who=all", "--signal=SIGKILL", unit])
            .status();
        let _ = Command::new("systemctl")
            .args(["--user", "stop", unit])
            .status();
    } else if child
        .child
        .try_wait()
        .expect("poll XP after grace period")
        .is_none()
    {
        child.child.kill().expect("kill XP after grace period");
    }
    let _ = child.child.wait();
}

pub async fn run_source_delivery_journal_resource_workload(binary: &Path) -> u64 {
    let temp = tempfile::tempdir().expect("source journal resource data directory");
    let bind_port = reserve_local_port();
    run_init(binary, temp.path(), bind_port);
    let cluster = ClusterMetadata::load(temp.path()).expect("load source journal cluster");
    prepare_source_delivery_storage(temp.path(), &cluster);
    let log_path = temp.path().join("source-journal.log");
    let mut child = spawn_xp(binary, temp.path(), bind_port, "source-journal");
    wait_for_xp(&mut child, bind_port, &log_path).await;
    let pid = child.id();
    assert_expected_memory_scope(pid);

    let ca_pem = cluster
        .read_cluster_ca_pem(temp.path())
        .expect("read source journal CA");
    let ca_key_pem = cluster
        .read_cluster_ca_key_pem(temp.path())
        .expect("read source journal CA key")
        .expect("source journal private CA key");
    let uri: axum::http::Uri = "/api/admin/_internal/history-repository/status"
        .parse()
        .expect("source journal status URI");
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("source journal HTTP client");
    // Warm the endpoint and allocator before taking the bounded-cycle samples.
    let initial_pending =
        request_source_status(&client, bind_port, &uri, &cluster, &ca_key_pem, &ca_pem).await;
    let baseline_rss = read_process_rss_bytes(pid);
    let mut peak_pss_kib = read_pss(pid).expect("read source journal XP PSS").total_kib;
    let mut cpu_percentages = Vec::with_capacity(8);
    let mut max_read_bytes = 0_u64;
    let mut max_rss_delta = 0_u64;
    let mut saw_replayed_page = false;
    let stop_sampling = Arc::new(AtomicBool::new(false));
    let sampled_stop = stop_sampling.clone();
    let pss_peak = Arc::new(AtomicU64::new(peak_pss_kib));
    let sampled_pss_peak = pss_peak.clone();
    let sampler = tokio::spawn(async move {
        while !sampled_stop.load(Ordering::Relaxed) {
            if let Some(sample) = read_pss(pid) {
                sampled_pss_peak.fetch_max(sample.total_kib, Ordering::Relaxed);
            }
            sleep(Duration::from_millis(5)).await;
        }
    });
    for _ in 0..8 {
        let started = Instant::now();
        let cpu_before = read_cpu_ticks(pid);
        let read_before = read_process_read_bytes(pid);
        let pending =
            request_source_status(&client, bind_port, &uri, &cluster, &ca_key_pem, &ca_pem).await;
        if pending < initial_pending {
            saw_replayed_page = true;
        }
        let operation_elapsed = started.elapsed();
        if operation_elapsed < Duration::from_secs(1) {
            sleep(Duration::from_secs(1) - operation_elapsed).await;
        }
        let wall_seconds = started.elapsed().as_secs_f64();
        let ticks_per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }
            .try_into()
            .ok()
            .filter(|value: &u64| *value > 0)
            .unwrap_or(100);
        let cpu_seconds =
            read_cpu_ticks(pid).saturating_sub(cpu_before) as f64 / ticks_per_second as f64;
        cpu_percentages.push(cpu_seconds * 100.0 / wall_seconds);
        max_read_bytes =
            max_read_bytes.max(read_process_read_bytes(pid).saturating_sub(read_before));
        max_rss_delta = max_rss_delta.max(read_process_rss_bytes(pid).saturating_sub(baseline_rss));
        peak_pss_kib =
            peak_pss_kib.max(read_pss(pid).expect("read source journal XP PSS").total_kib);
    }
    let replay_deadline = Instant::now() + Duration::from_secs(75);
    while !saw_replayed_page && Instant::now() < replay_deadline {
        let pending =
            request_source_status(&client, bind_port, &uri, &cluster, &ca_key_pem, &ca_pem).await;
        saw_replayed_page = pending < initial_pending;
        peak_pss_kib =
            peak_pss_kib.max(read_pss(pid).expect("read source journal XP PSS").total_kib);
        sleep(Duration::from_secs(1)).await;
    }
    stop_sampling.store(true, Ordering::Relaxed);
    sampler.await.expect("source journal PSS sampler");
    peak_pss_kib = peak_pss_kib.max(pss_peak.load(Ordering::Relaxed));
    assert!(
        saw_replayed_page,
        "source journal worker must replay at least one bounded page"
    );
    cpu_percentages.sort_by(f64::total_cmp);
    let cpu_p95 = *cpu_percentages
        .last()
        .expect("source journal resource samples");
    println!(
        "source_journal_resource cpu_p95_percent={cpu_p95:.2} max_read_bytes={max_read_bytes} \
         max_rss_delta={max_rss_delta} peak_pss_kib={peak_pss_kib}"
    );
    assert!(
        cpu_p95 <= JOURNAL_CPU_P95_LIMIT_PERCENT,
        "source journal CPU p95 {cpu_p95:.2}% exceeds {JOURNAL_CPU_P95_LIMIT_PERCENT}%"
    );
    assert!(
        max_read_bytes <= JOURNAL_READ_BYTES_LIMIT,
        "source journal read_bytes {max_read_bytes} exceeds {JOURNAL_READ_BYTES_LIMIT}"
    );
    assert!(
        max_rss_delta <= JOURNAL_RSS_DELTA_LIMIT,
        "source journal RSS delta {max_rss_delta} exceeds {JOURNAL_RSS_DELTA_LIMIT}"
    );
    println!("source_journal_resource_candidate_peak_pss_kib={peak_pss_kib}");
    stop_child(&mut child).await;
    peak_pss_kib
}

async fn request_source_status(
    client: &reqwest::Client,
    bind_port: u16,
    uri: &axum::http::Uri,
    cluster: &ClusterMetadata,
    ca_key_pem: &str,
    ca_pem: &str,
) -> u64 {
    let context = xp::internal_auth::RequestContext::now(
        xp::internal_auth::InternalRoute::MeshV2,
        &cluster.cluster_id,
        &cluster.node_id,
        &cluster.node_id,
        xp::id::new_ulid_string(),
    );
    let mut headers = axum::http::HeaderMap::new();
    xp::internal_auth::sign_request_v2(
        ca_key_pem,
        ca_pem,
        &axum::http::Method::GET,
        uri,
        None,
        &[],
        &context,
        &mut headers,
    )
    .expect("sign source journal status request");
    let mut request = client.get(format!("http://127.0.0.1:{bind_port}{uri}"));
    for (name, value) in &headers {
        request = request.header(name.as_str(), value.to_str().expect("signed header value"));
    }
    let response = request
        .send()
        .await
        .expect("source journal status response");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let status: serde_json::Value = response.json().await.expect("decode source journal status");
    status["source_delivery"]["pending_segments"]
        .as_u64()
        .expect("source journal pending segment count")
}

fn prepare_source_delivery_storage(data_dir: &Path, cluster: &ClusterMetadata) {
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
    let state_payload = connection
        .query_row(
            "SELECT payload FROM history_snapshots WHERE key = 'persistent_state'",
            [],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .expect("read initialized source journal state snapshot");
    let mut state: serde_json::Value =
        serde_json::from_slice(&state_payload).expect("decode source journal state snapshot");
    state["repository_membership"] = serde_json::json!({
        "members": [{
            "identity": {
                "node_id": cluster.node_id.clone(),
                "ed25519_public_key": URL_SAFE_NO_PAD.encode([7_u8; 32]),
                "x25519_relay_public_key": URL_SAFE_NO_PAD.encode([8_u8; 32])
            },
            "lifecycle": "ready",
            "catch_up_completed_at": 1,
            "ready_at": 301,
            "replica_converged": false,
            "capacity": {
                "quota_bytes": 10_u64 * 1024 * 1024 * 1024,
                "used_bytes": 0,
                "filesystem_available_bytes": u64::MAX
            }
        }]
    });
    let state_payload = serde_json::to_vec(&state).expect("encode source journal state snapshot");
    connection
        .execute(
            "UPDATE history_snapshots SET payload = ?1 WHERE key = 'persistent_state'",
            rusqlite::params![state_payload],
        )
        .expect("write source journal repository membership");
}
