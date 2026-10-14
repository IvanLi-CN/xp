use super::*;
use ed25519_dalek::{Signer as _, SigningKey};
use prost::Message as _;
use sha2::{Digest as _, Sha256};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

// The replay worker may use one CPU while draining a durable backlog. Keep a one-core ceiling,
// bounded RSS growth, and the 128 MiB/no-swap cgroup as independent safety gates.
const JOURNAL_CPU_P95_LIMIT_PERCENT: f64 = 100.0;
const JOURNAL_READ_BYTES_LIMIT: u64 = 4 * 1024 * 1024;
const JOURNAL_CYCLE_READ_BYTES_LIMIT: u64 = 16 * 1024 * 1024;
// SQLite's WAL/page cache and the bounded replay window may fault in resident pages while the
// process stays well below the cgroup/PSS gate. Keep a fixed 32 MiB diagnostic ceiling so this
// check catches unbounded growth without rejecting the measured bounded working set.
const JOURNAL_RSS_DELTA_LIMIT: u64 = 32 * 1024 * 1024;
const JOURNAL_THROUGHPUT_SAMPLE_SECONDS: u64 = 5 * 60;
// Keep the 20,000-segment count cap while leaving headroom for XP, SQLite and the
// no-swap cgroup. The generated fixture is smaller than the backlog row so the
// byte-based ten-x gate can drain from the same bounded synthetic journal.
const JOURNAL_BACKLOG_PAYLOAD_BYTES: usize = 3 * 1024;
const JOURNAL_FIXTURE_PAYLOAD_BYTES: usize = 2 * 1024;

#[derive(Debug, Clone)]
struct DeliveryWindowSnapshot {
    elapsed_seconds: u64,
    generated_segments: u64,
    generated_bytes: u64,
    peak_generated_segments: u64,
    peak_generated_bytes: u64,
    acknowledged_segments: u64,
    acknowledged_bytes: u64,
    applied_segments: u64,
    applied_bytes: u64,
    source_ack_meets_ten_x_segments: bool,
    source_ack_meets_ten_x_bytes: bool,
    repository_apply_meets_ten_x_segments: bool,
    repository_apply_meets_ten_x_bytes: bool,
    meets_ten_x: bool,
}

#[derive(Debug, Clone)]
struct DeliveryScopeSnapshot {
    target_repository_id: String,
    last_completed_window: Option<DeliveryWindowSnapshot>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct FixtureCursor {
    #[prost(string, tag = "1")]
    source_node_id: String,
    #[prost(uint64, tag = "2")]
    source_epoch: u64,
    #[prost(string, tag = "3")]
    stream: String,
    #[prost(uint64, tag = "4")]
    sequence: u64,
}

#[derive(Clone, PartialEq, prost::Message)]
struct FixtureRecord {
    #[prost(string, tag = "1")]
    schema_id: String,
    #[prost(uint32, tag = "2")]
    schema_version: u32,
    #[prost(bytes, tag = "3")]
    record_key: Vec<u8>,
    #[prost(bytes, tag = "4")]
    payload: Vec<u8>,
    #[prost(bool, tag = "5")]
    tombstone: bool,
    #[prost(string, tag = "6")]
    subject_node_id: String,
    #[prost(string, tag = "7")]
    observer_node_id: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct FixtureSegment {
    #[prost(string, tag = "1")]
    cluster_id: String,
    #[prost(message, optional, tag = "2")]
    first_cursor: Option<FixtureCursor>,
    #[prost(message, optional, tag = "3")]
    last_cursor: Option<FixtureCursor>,
    #[prost(message, repeated, tag = "4")]
    records: Vec<FixtureRecord>,
    #[prost(bytes, tag = "5")]
    previous_segment_hash: Vec<u8>,
    #[prost(uint64, tag = "6")]
    opened_at_unix_seconds: u64,
    #[prost(uint64, tag = "7")]
    closed_at_unix_seconds: u64,
    #[prost(bytes, tag = "8")]
    signature: Vec<u8>,
    #[prost(bytes, tag = "9")]
    records_hash: Vec<u8>,
}

fn signed_fixture_wire(
    signing_key: &SigningKey,
    cluster_id: &str,
    source_node_id: &str,
    sequence: u64,
    payload: &[u8],
    previous_hash: Option<[u8; 32]>,
) -> (Vec<u8>, [u8; 32], usize) {
    let record = FixtureRecord {
        schema_id: "runtime.v1".to_owned(),
        schema_version: 1,
        record_key: format!("runtime:{sequence}").into_bytes(),
        payload: payload.to_vec(),
        tombstone: false,
        subject_node_id: source_node_id.to_owned(),
        observer_node_id: source_node_id.to_owned(),
    };
    let mut records_hash = Sha256::new();
    let mut encoded_record = Vec::new();
    record
        .encode_length_delimited(&mut encoded_record)
        .expect("encode source journal record");
    records_hash.update(encoded_record);
    let mut canonical = FixtureSegment {
        first_cursor: Some(FixtureCursor {
            source_node_id: source_node_id.to_owned(),
            source_epoch: 1,
            stream: "runtime".to_owned(),
            sequence,
        }),
        last_cursor: Some(FixtureCursor {
            source_node_id: source_node_id.to_owned(),
            source_epoch: 1,
            stream: "runtime".to_owned(),
            sequence,
        }),
        records: vec![record],
        previous_segment_hash: previous_hash.map_or_else(Vec::new, |hash| hash.to_vec()),
        opened_at_unix_seconds: 100,
        closed_at_unix_seconds: 100,
        records_hash: records_hash.finalize().to_vec(),
        ..Default::default()
    };
    canonical.cluster_id = cluster_id.to_owned();
    let canonical_wire = canonical.encode_to_vec();
    let signature = signing_key.sign(&canonical_wire).to_bytes().to_vec();
    let mut signed = canonical;
    signed.signature = signature;
    let wire = signed.encode_to_vec();
    let mut segment_hash = Sha256::new();
    let canonical_len = canonical_wire.len();
    segment_hash.update(&canonical_wire);
    segment_hash.update(&signed.signature);
    (wire, segment_hash.finalize().into(), canonical_len)
}

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

fn derived_fixture_key_material(
    context: &[u8],
    cluster: &ClusterMetadata,
    ca_key_pem: &str,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(context);
    hasher.update(cluster.cluster_id.as_bytes());
    hasher.update([0]);
    hasher.update(cluster.node_id.as_bytes());
    hasher.update([0]);
    hasher.update(ca_key_pem.as_bytes());
    hasher.finalize().into()
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
    unsafe {
        std::env::set_var(
            "XP_RESOURCE_FIXTURE_PAYLOAD_BYTES",
            JOURNAL_FIXTURE_PAYLOAD_BYTES.to_string(),
        );
    }
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
    let initial_status =
        request_source_status(&client, bind_port, &uri, &cluster, &ca_key_pem, &ca_pem).await;
    // Touch the status path twice before taking the RSS baseline so lazy SQLite/HTTP pages are
    // accounted for as warm process state rather than charged to the replay workload.
    let _ = request_source_status(&client, bind_port, &uri, &cluster, &ca_key_pem, &ca_pem).await;
    sleep(Duration::from_secs(1)).await;
    let baseline_rss = read_process_rss_bytes(pid);
    let mut peak_pss_kib = read_pss(pid).expect("read source journal XP PSS").total_kib;
    let mut cpu_percentages = Vec::with_capacity(JOURNAL_THROUGHPUT_SAMPLE_SECONDS as usize);
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
    let sample_window_started = Instant::now();
    let initial_pending = initial_status.pending_segments;
    let initial_metrics = initial_status.metrics;
    let mut max_cycle_read_bytes = 0_u64;
    let mut cycle_read_before = read_process_read_bytes(pid);
    let mut cycle_started = Instant::now();
    for _second in 0..JOURNAL_THROUGHPUT_SAMPLE_SECONDS {
        let started = Instant::now();
        let cpu_before = read_cpu_ticks(pid);
        let read_before = read_process_read_bytes(pid);
        let status =
            request_source_status(&client, bind_port, &uri, &cluster, &ca_key_pem, &ca_pem).await;
        let pending = status.pending_segments;
        eprintln!("source journal warm status pending={pending}");
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
        if cycle_started.elapsed() >= Duration::from_secs(5) {
            let cycle_read = read_process_read_bytes(pid);
            max_cycle_read_bytes =
                max_cycle_read_bytes.max(cycle_read.saturating_sub(cycle_read_before));
            cycle_read_before = cycle_read;
            cycle_started = Instant::now();
        }
    }
    let replay_deadline = Instant::now() + Duration::from_secs(75);
    while !saw_replayed_page && Instant::now() < replay_deadline {
        let started = Instant::now();
        let cpu_before = read_cpu_ticks(pid);
        let read_before = read_process_read_bytes(pid);
        let status =
            request_source_status(&client, bind_port, &uri, &cluster, &ca_key_pem, &ca_pem).await;
        let pending = status.pending_segments;
        eprintln!("source journal replay status pending={pending}");
        saw_replayed_page = pending < initial_pending;
        let operation_elapsed = started.elapsed();
        let wall_seconds = operation_elapsed.as_secs_f64().max(0.001);
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
        if operation_elapsed < Duration::from_secs(1) {
            sleep(Duration::from_secs(1) - operation_elapsed).await;
        }
        if cycle_started.elapsed() >= Duration::from_secs(5) {
            let cycle_read = read_process_read_bytes(pid);
            max_cycle_read_bytes =
                max_cycle_read_bytes.max(cycle_read.saturating_sub(cycle_read_before));
            cycle_read_before = cycle_read;
            cycle_started = Instant::now();
        }
    }
    let cycle_read = read_process_read_bytes(pid);
    max_cycle_read_bytes = max_cycle_read_bytes.max(cycle_read.saturating_sub(cycle_read_before));
    stop_sampling.store(true, Ordering::Relaxed);
    sampler.await.expect("source journal PSS sampler");
    peak_pss_kib = peak_pss_kib.max(pss_peak.load(Ordering::Relaxed));
    if !saw_replayed_page {
        let log = fs::read_to_string(&log_path)
            .unwrap_or_else(|error| format!("<unavailable: {error}>",));
        eprintln!("source journal XP log before replay failure:\n{log}");
    }
    assert!(
        saw_replayed_page,
        "source journal worker must replay at least one bounded page"
    );
    let post_replay_status =
        request_source_status(&client, bind_port, &uri, &cluster, &ca_key_pem, &ca_pem).await;
    let post_replay_pending = post_replay_status.pending_segments;
    assert!(
        post_replay_pending < initial_pending,
        "source journal backlog must remain drained after the replay page"
    );
    let completed_window = post_replay_status
        .metrics
        .last_completed_window
        .as_ref()
        .expect("status must expose the completed 300-second delivery window");
    assert_eq!(
        completed_window.elapsed_seconds, JOURNAL_THROUGHPUT_SAMPLE_SECONDS,
        "delivery window must be exactly five minutes"
    );
    eprintln!(
        concat!(
            "source journal window generated={} / {} bytes, peak={} / {} bytes, ",
            "ack={} / {} bytes, apply={} / {} bytes, source_ack={} / {}, ",
            "repository_apply={} / {}"
        ),
        completed_window.generated_segments,
        completed_window.generated_bytes,
        completed_window.peak_generated_segments,
        completed_window.peak_generated_bytes,
        completed_window.acknowledged_segments,
        completed_window.acknowledged_bytes,
        completed_window.applied_segments,
        completed_window.applied_bytes,
        completed_window.source_ack_meets_ten_x_segments,
        completed_window.source_ack_meets_ten_x_bytes,
        completed_window.repository_apply_meets_ten_x_segments,
        completed_window.repository_apply_meets_ten_x_bytes,
    );
    if !completed_window.meets_ten_x {
        let log = fs::read_to_string(&log_path)
            .unwrap_or_else(|error| format!("<unavailable: {error}>",));
        eprintln!("source journal XP log before ten-x failure:\n{log}");
    }
    assert!(
        completed_window.meets_ten_x,
        "aggregate completed delivery window must meet the ten-x gate"
    );
    let scoped_window = post_replay_status
        .metrics
        .delivery_scopes
        .iter()
        .find(|scope| {
            scope.target_repository_id == cluster.node_id && scope.last_completed_window.is_some()
        })
        .and_then(|scope| scope.last_completed_window.as_ref())
        .expect("status must expose the local target scope completed window");
    assert_eq!(
        scoped_window.elapsed_seconds,
        JOURNAL_THROUGHPUT_SAMPLE_SECONDS
    );
    assert!(
        scoped_window.meets_ten_x,
        "local target scope must meet the ten-x gate"
    );
    cpu_percentages.sort_by(f64::total_cmp);
    let p95_index = (cpu_percentages.len().saturating_sub(1) * 95) / 100;
    let cpu_p95 = cpu_percentages[p95_index];
    let elapsed_seconds = sample_window_started.elapsed().as_secs().max(1);
    let generated_segments = post_replay_status
        .metrics
        .generated_segments
        .saturating_sub(initial_metrics.generated_segments);
    let generated_bytes = post_replay_status
        .metrics
        .generated_bytes
        .saturating_sub(initial_metrics.generated_bytes);
    let acknowledged_segments = post_replay_status
        .metrics
        .acknowledged_segments
        .saturating_sub(initial_metrics.acknowledged_segments);
    let acknowledged_bytes = post_replay_status
        .metrics
        .acknowledged_bytes
        .saturating_sub(initial_metrics.acknowledged_bytes);
    let applied_segments = post_replay_status
        .metrics
        .applied_segments
        .saturating_sub(initial_metrics.applied_segments);
    let applied_bytes = post_replay_status
        .metrics
        .applied_bytes
        .saturating_sub(initial_metrics.applied_bytes);
    println!(
        "source_journal_resource cpu_p95_percent={cpu_p95:.2} max_read_bytes={max_read_bytes} \
         max_cycle_read_bytes={max_cycle_read_bytes} max_rss_delta={max_rss_delta} \
         peak_pss_kib={peak_pss_kib} sample_window_seconds={elapsed_seconds} \
         generated_segments={generated_segments} generated_bytes={generated_bytes} \
         peak_generated_segments={} peak_generated_bytes={} \
         acknowledged_segments={acknowledged_segments} acknowledged_bytes={acknowledged_bytes} \
         applied_segments={applied_segments} applied_bytes={applied_bytes}",
        completed_window.peak_generated_segments, completed_window.peak_generated_bytes
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
        max_cycle_read_bytes <= JOURNAL_CYCLE_READ_BYTES_LIMIT,
        "source journal cycle read_bytes {max_cycle_read_bytes} exceeds \
         {JOURNAL_CYCLE_READ_BYTES_LIMIT}"
    );
    assert!(
        max_rss_delta <= JOURNAL_RSS_DELTA_LIMIT,
        "source journal RSS delta {max_rss_delta} exceeds {JOURNAL_RSS_DELTA_LIMIT}"
    );
    assert!(
        generated_segments > 0 && generated_bytes > 0,
        concat!(
            "source journal durable counters did not record the real fixture enqueue path: ",
            "generated={}/{}, ack={}/{}, apply={}/{}"
        ),
        generated_segments,
        generated_bytes,
        acknowledged_segments,
        acknowledged_bytes,
        applied_segments,
        applied_bytes
    );
    println!("source_journal_resource_candidate_peak_pss_kib={peak_pss_kib}");
    stop_child(&mut child).await;
    unsafe {
        std::env::remove_var("XP_RESOURCE_FIXTURE_PAYLOAD_BYTES");
    }
    peak_pss_kib
}

#[derive(Debug, Clone)]
struct SourceDeliveryMetricsSnapshot {
    generated_segments: u64,
    generated_bytes: u64,
    peak_generated_segments: u64,
    peak_generated_bytes: u64,
    acknowledged_segments: u64,
    acknowledged_bytes: u64,
    applied_segments: u64,
    applied_bytes: u64,
    last_completed_window: Option<DeliveryWindowSnapshot>,
    delivery_scopes: Vec<DeliveryScopeSnapshot>,
}

#[derive(Debug, Clone)]
struct SourceStatusSnapshot {
    pending_segments: u64,
    metrics: SourceDeliveryMetricsSnapshot,
}

async fn request_source_status(
    client: &reqwest::Client,
    bind_port: u16,
    uri: &axum::http::Uri,
    cluster: &ClusterMetadata,
    ca_key_pem: &str,
    ca_pem: &str,
) -> SourceStatusSnapshot {
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
    let metrics = &status["delivery_metrics"];
    let parse_window = |value: &serde_json::Value| {
        (!value.is_null()).then(|| DeliveryWindowSnapshot {
            elapsed_seconds: value["elapsed_seconds"]
                .as_u64()
                .expect("delivery window elapsed seconds"),
            generated_segments: value["generated_segments"]
                .as_u64()
                .expect("delivery window generated segments"),
            generated_bytes: value["generated_bytes"]
                .as_u64()
                .expect("delivery window generated bytes"),
            peak_generated_segments: value["peak_generated_segments"]
                .as_u64()
                .expect("delivery window peak generated segments"),
            peak_generated_bytes: value["peak_generated_bytes"]
                .as_u64()
                .expect("delivery window peak generated bytes"),
            acknowledged_segments: value["acknowledged_segments"]
                .as_u64()
                .expect("delivery window acknowledged segments"),
            acknowledged_bytes: value["acknowledged_bytes"]
                .as_u64()
                .expect("delivery window acknowledged bytes"),
            applied_segments: value["applied_segments"]
                .as_u64()
                .expect("delivery window applied segments"),
            applied_bytes: value["applied_bytes"]
                .as_u64()
                .expect("delivery window applied bytes"),
            source_ack_meets_ten_x_segments: value["source_ack_meets_ten_x_segments"]
                .as_bool()
                .expect("delivery window source ACK segment result"),
            source_ack_meets_ten_x_bytes: value["source_ack_meets_ten_x_bytes"]
                .as_bool()
                .expect("delivery window source ACK byte result"),
            repository_apply_meets_ten_x_segments: value["repository_apply_meets_ten_x_segments"]
                .as_bool()
                .expect("delivery window repository apply segment result"),
            repository_apply_meets_ten_x_bytes: value["repository_apply_meets_ten_x_bytes"]
                .as_bool()
                .expect("delivery window repository apply byte result"),
            meets_ten_x: value["meets_ten_x"]
                .as_bool()
                .expect("delivery ten-x result"),
        })
    };
    let delivery_scopes = metrics["delivery_scopes"]
        .as_array()
        .expect("delivery scope metrics")
        .iter()
        .map(|scope| DeliveryScopeSnapshot {
            target_repository_id: scope["target_repository_id"]
                .as_str()
                .expect("delivery scope target")
                .to_owned(),
            last_completed_window: parse_window(&scope["last_completed_window"]),
        })
        .collect();
    SourceStatusSnapshot {
        pending_segments: status["source_delivery"]["pending_segments"]
            .as_u64()
            .expect("source journal pending segment count"),
        metrics: SourceDeliveryMetricsSnapshot {
            generated_segments: metrics["generated_segments"]
                .as_u64()
                .expect("generated segment counter"),
            generated_bytes: metrics["generated_bytes"]
                .as_u64()
                .expect("generated byte counter"),
            peak_generated_segments: metrics["peak_generated_segments"]
                .as_u64()
                .expect("peak generated segment counter"),
            peak_generated_bytes: metrics["peak_generated_bytes"]
                .as_u64()
                .expect("peak generated byte counter"),
            acknowledged_segments: metrics["acknowledged_segments"]
                .as_u64()
                .expect("acknowledged segment counter"),
            acknowledged_bytes: metrics["acknowledged_bytes"]
                .as_u64()
                .expect("acknowledged byte counter"),
            applied_segments: metrics["applied_segments"]
                .as_u64()
                .expect("applied segment counter"),
            applied_bytes: metrics["applied_bytes"]
                .as_u64()
                .expect("applied byte counter"),
            last_completed_window: parse_window(&metrics["last_completed_window"]),
            delivery_scopes,
        },
    }
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
    let ca_key_pem = cluster
        .read_cluster_ca_key_pem(data_dir)
        .expect("read source journal CA key")
        .expect("source journal private CA key");
    let signing_key = SigningKey::from_bytes(&derived_fixture_key_material(
        b"xp-history-repository-ed25519-v1\0",
        cluster,
        &ca_key_pem,
    ));
    let public_key = URL_SAFE_NO_PAD.encode(signing_key.verifying_key().to_bytes());
    let relay_secret = StaticSecret::from(derived_fixture_key_material(
        b"xp-history-repository-x25519-v1\0",
        cluster,
        &ca_key_pem,
    ));
    let relay_public_key = URL_SAFE_NO_PAD.encode(X25519PublicKey::from(&relay_secret).as_bytes());
    let identity = serde_json::to_vec(&serde_json::json!({
        "node_id": cluster.node_id.clone(),
        "ed25519_public_key": public_key.clone(),
        "x25519_relay_public_key": relay_public_key.clone(),
    }))
    .expect("encode source journal identity");
    // Keep the 20,000-row backlog within the 128 MiB capacity boundary while leaving
    // enough canonical bytes for the real enqueue path to prove both ten-x counters.
    let payload = vec![0_u8; JOURNAL_BACKLOG_PAYLOAD_BYTES];
    let mut previous_hash = None;
    let mut total_wire_bytes = 0_i64;
    for batch_start in (0..20_000_i64).step_by(256) {
        let transaction = connection
            .unchecked_transaction()
            .expect("begin source journal backlog transaction");
        for sequence in batch_start..(batch_start + 256).min(20_000) {
            let (wire, segment_hash, _) = signed_fixture_wire(
                &signing_key,
                &cluster.cluster_id,
                &cluster.node_id,
                sequence as u64,
                &payload,
                previous_hash,
            );
            previous_hash = Some(segment_hash);
            let journal_id = format!("{:x}", Sha256::digest(&wire));
            total_wire_bytes = total_wire_bytes
                .checked_add(i64::try_from(wire.len()).expect("source journal wire length"))
                .expect("source journal byte count");
            transaction
                .execute(
                    "INSERT INTO source_delivery_journal
                         (id, stream, closed_at, identity, wire, created_at,
                          source_node_id, source_epoch, first_sequence)
                     VALUES (?1, 'runtime', 100, ?2, ?3, 100, ?4, 1, ?5)",
                    rusqlite::params![journal_id, &identity, &wire, &cluster.node_id, sequence,],
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
            [total_wire_bytes],
        )
        .expect("record source journal resource backlog");
    connection
        .execute(
            "INSERT OR REPLACE INTO source_delivery_journal_stream_state
             (stream, pending_segments) VALUES ('runtime', 20_000)",
            [],
        )
        .expect("record source journal stream backlog");
    let window_started_at = chrono::Utc::now().timestamp();
    connection
        .execute(
            "UPDATE repository_delivery_metrics
             SET window_started_at = ?1,
                 window_generated_segments = 0,
                 window_generated_bytes = 0,
                 window_acknowledged_segments = 0,
                 window_acknowledged_bytes = 0,
                 window_applied_segments = 0,
                 window_applied_bytes = 0,
                 completed_window_started_at = NULL,
                 completed_window_ended_at = NULL,
                 completed_generated_segments = NULL,
                 completed_generated_bytes = NULL,
                 completed_acknowledged_segments = NULL,
                 completed_acknowledged_bytes = NULL,
                 completed_applied_segments = NULL,
                 completed_applied_bytes = NULL
             WHERE singleton = 1",
            [window_started_at],
        )
        .expect("seed source journal five-minute window");
    for target_repository_id in ["", cluster.node_id.as_str()] {
        connection
            .execute(
                "INSERT OR REPLACE INTO repository_delivery_metric_scopes
                 (source_node_id, stream, target_repository_id,
                  generated_segments, generated_bytes,
                  acknowledged_segments, acknowledged_bytes,
                  applied_segments, applied_bytes)
                 VALUES (?1, 'runtime', ?2, 0, 0, 0, 0, 0, 0)",
                rusqlite::params![cluster.node_id, target_repository_id],
            )
            .expect("seed source journal delivery scope");
        connection
            .execute(
                "INSERT OR REPLACE INTO repository_delivery_metric_scope_windows
                 (source_node_id, stream, target_repository_id,
                  window_started_at, window_generated_segments, window_generated_bytes,
                  window_acknowledged_segments, window_acknowledged_bytes,
                  window_applied_segments, window_applied_bytes)
                 VALUES (?1, 'runtime', ?2, ?3, 0, 0, 0, 0, 0, 0)",
                rusqlite::params![cluster.node_id, target_repository_id, window_started_at],
            )
            .expect("seed source journal scope window");
    }
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
                "ed25519_public_key": public_key,
                "x25519_relay_public_key": relay_public_key
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
    let mut store = JsonSnapshotStore::load_or_init(StoreInit {
        data_dir: data_dir.to_path_buf(),
        bootstrap_node_id: Some(cluster.node_id.clone()),
        bootstrap_node_name: cluster.node_name.clone(),
        bootstrap_access_host: cluster.access_host.clone(),
        bootstrap_api_base_url: cluster.api_base_url.clone(),
    })
    .expect("load source journal control state");
    store.state_mut().repository_membership = Some(
        serde_json::from_value(state["repository_membership"].clone())
            .expect("decode source journal repository membership"),
    );
    store.save().expect("persist source journal control state");
    let persisted = JsonSnapshotStore::load_or_init(StoreInit {
        data_dir: data_dir.to_path_buf(),
        bootstrap_node_id: Some(cluster.node_id.clone()),
        bootstrap_node_name: cluster.node_name.clone(),
        bootstrap_access_host: cluster.access_host.clone(),
        bootstrap_api_base_url: cluster.api_base_url.clone(),
    })
    .expect("reload source journal control state");
    assert!(
        persisted.state().nodes.contains_key(&cluster.node_id),
        "source journal fixture must reload local node metadata"
    );
}
