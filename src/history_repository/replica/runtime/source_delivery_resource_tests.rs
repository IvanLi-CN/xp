#[cfg(target_os = "linux")]
use std::{
    fs, thread,
    time::{Duration, Instant},
};

#[cfg(target_os = "linux")]
fn process_cpu_ticks() -> u64 {
    let stat = fs::read_to_string("/proc/self/stat").expect("read process stat");
    let fields = stat
        .split_once(") ")
        .expect("process stat comm delimiter")
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    let user = fields[11].parse::<u64>().expect("user CPU ticks");
    let system = fields[12].parse::<u64>().expect("system CPU ticks");
    user + system
}

#[cfg(target_os = "linux")]
fn process_read_bytes() -> u64 {
    fs::read_to_string("/proc/self/io")
        .expect("read process I/O")
        .lines()
        .find_map(|line| line.strip_prefix("read_bytes:")?.trim().parse().ok())
        .expect("read process read_bytes")
}

#[cfg(target_os = "linux")]
fn process_rss_bytes() -> u64 {
    let resident_pages = fs::read_to_string("/proc/self/statm")
        .expect("read process statm")
        .split_whitespace()
        .nth(1)
        .expect("resident page count")
        .parse::<u64>()
        .expect("parse resident page count");
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    assert!(page_size > 0, "system page size must be positive");
    resident_pages.saturating_mul(page_size as u64)
}

#[cfg(target_os = "linux")]
#[test]
fn source_delivery_journal_resource_budget_stays_fixed_for_large_backlog() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    drop(storage);
    let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("open history database");
    let identity = serde_json::to_vec(&super::identity()).expect("serialize source identity");
    let wire = vec![0_u8; 128];
    let transaction = connection
        .unchecked_transaction()
        .expect("begin backlog transaction");
    for sequence in 0..20_000_i64 {
        transaction
            .execute(
                "INSERT INTO source_delivery_journal
                     (id, stream, closed_at, identity, wire, created_at,
                      source_node_id, source_epoch, first_sequence)
                 VALUES (?1, 'runtime', 100, ?2, ?3, 100, 'node-a', 1, ?4)",
                rusqlite::params![
                    format!("resource-segment-{sequence}"),
                    &identity,
                    &wire,
                    sequence
                ],
            )
            .expect("insert resource backlog row");
    }
    transaction
        .execute(
            "UPDATE source_delivery_journal_state
             SET pending_segments = 20_000, pending_bytes = ?1,
                 epoch_high_water = 1, order_repair_completed = 1
             WHERE singleton = 1",
            [wire.len() as i64 * 20_000],
        )
        .expect("record resource backlog statistics");
    transaction.commit().expect("commit resource backlog");
    drop(connection);

    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let _ = storage
        .source_delivery_journal_summary()
        .expect("warm summary");
    let _ = storage
        .source_delivery_journal_page(256)
        .expect("warm page");
    let baseline_rss = process_rss_bytes();
    let ticks_per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    assert!(ticks_per_second > 0, "CPU tick rate must be positive");
    let mut cpu_percentages = Vec::with_capacity(5);
    let mut max_read_bytes = 0_u64;
    let mut max_rss_delta = 0_u64;

    for _ in 0..5 {
        let started = Instant::now();
        let cpu_before = process_cpu_ticks();
        let read_before = process_read_bytes();
        {
            let summary = storage
                .source_delivery_journal_summary()
                .expect("read bounded source summary");
            assert_eq!(summary.pending_segments, 20_000);
            let page = storage
                .source_delivery_journal_page(usize::MAX)
                .expect("read bounded source page");
            match page {
                crate::state::history_storage::SourceDeliveryJournalPage::Ready(rows) => {
                    assert_eq!(rows.len(), 256)
                }
                crate::state::history_storage::SourceDeliveryJournalPage::Repairing => {
                    panic!("current backlog must not be repairing")
                }
            }
        }
        let operation_elapsed = started.elapsed();
        if operation_elapsed < Duration::from_secs(1) {
            thread::sleep(Duration::from_secs(1) - operation_elapsed);
        }
        let wall_seconds = started.elapsed().as_secs_f64();
        let cpu_ticks = process_cpu_ticks().saturating_sub(cpu_before);
        let cpu_percent = (cpu_ticks as f64 / ticks_per_second as f64) * 100.0 / wall_seconds;
        cpu_percentages.push(cpu_percent);
        max_read_bytes = max_read_bytes.max(process_read_bytes().saturating_sub(read_before));
        max_rss_delta = max_rss_delta.max(process_rss_bytes().saturating_sub(baseline_rss));
    }

    cpu_percentages.sort_by(f64::total_cmp);
    let cpu_p95 = *cpu_percentages.last().expect("resource samples");
    println!(
        "source_journal_resource cpu_p95_percent={cpu_p95:.2} max_read_bytes={max_read_bytes} \
         max_rss_delta={max_rss_delta}"
    );
    assert!(
        cpu_p95 <= 9.0,
        "source journal CPU p95 {cpu_p95:.2}% exceeds 9%"
    );
    assert!(
        max_read_bytes <= 4 * 1024 * 1024,
        "source journal read_bytes {max_read_bytes} exceeds 4 MiB"
    );
    assert!(
        max_rss_delta <= 2 * 1024 * 1024,
        "source journal RSS delta {max_rss_delta} exceeds 2 MiB"
    );
}
