use rusqlite::Connection;
use tempfile::TempDir;

use super::*;
use crate::uptime_monitor::MonitorLifecycle;

fn observation(slot: u64) -> Observation {
    Observation {
        monitor_id: "monitor".to_owned(),
        revision: 1,
        observer_node_id: "node".to_owned(),
        slot_unix_seconds: slot,
        observed_at_unix_seconds: slot,
        outcome: ObservationOutcome::Success,
        error: None,
        latency_ms: Some(4),
        status_code: Some(200),
        packet_loss_percent: 0,
        ad_hoc: false,
    }
}

#[tokio::test]
async fn persists_pending_observations_before_history_delivery() {
    let temporary = TempDir::new().unwrap();
    let handle = UptimeHandle::load(temporary.path()).unwrap();
    handle.record(observation(60)).await.unwrap();
    handle.record(observation(60)).await.unwrap();
    let pending = handle.pending(10).await.unwrap();
    assert_eq!(pending.len(), 1);
    handle
        .mark_enqueued(&[pending[0].id.clone()])
        .await
        .unwrap();
    assert!(handle.pending(10).await.unwrap().is_empty());
}

#[tokio::test]
async fn ad_hoc_run_persists_its_terminal_observation() {
    let temporary = TempDir::new().unwrap();
    let handle = UptimeHandle::load(temporary.path()).unwrap();
    let run = AdHocRun {
        run_id: "run".to_owned(),
        monitor_id: "monitor".to_owned(),
        state: AdHocRunState::Queued,
        created_at_unix_seconds: 60,
        completed_at_unix_seconds: None,
        observation: None,
        reason: None,
    };
    handle.create_ad_hoc_run(&run).await.unwrap();
    handle.mark_ad_hoc_run_running("run").await.unwrap();
    let mut result = observation(61);
    result.ad_hoc = true;
    assert!(
        handle
            .record_with_id("run".to_owned(), result.clone())
            .await
            .unwrap()
    );
    handle.complete_ad_hoc_run("run", &result).await.unwrap();

    let saved = handle.ad_hoc_run("run").await.unwrap().unwrap();
    assert!(matches!(saved.state, AdHocRunState::Succeeded));
    assert_eq!(saved.observation, Some(result));
}

#[tokio::test]
async fn keeps_distinct_ad_hoc_runs_from_the_same_second() {
    let temporary = TempDir::new().unwrap();
    let handle = UptimeHandle::load(temporary.path()).unwrap();
    let mut first = observation(60);
    first.ad_hoc = true;
    let second = first.clone();

    assert!(
        handle
            .record_with_id("first-run".to_owned(), first)
            .await
            .unwrap()
    );
    assert!(
        handle
            .record_with_id("second-run".to_owned(), second)
            .await
            .unwrap()
    );
    assert_eq!(
        handle
            .observations("monitor", 0, 120, 10)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn migrates_legacy_slot_key_without_dropping_ad_hoc_observations() {
    let temporary = TempDir::new().unwrap();
    let path = temporary.path().join("uptime.sqlite3");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "
			CREATE TABLE uptime_observations (
				id TEXT PRIMARY KEY,
				monitor_id TEXT NOT NULL,
				revision INTEGER NOT NULL,
				observer_node_id TEXT NOT NULL,
				slot_unix_seconds INTEGER NOT NULL,
				observed_at_unix_seconds INTEGER NOT NULL,
				ad_hoc INTEGER NOT NULL,
				enqueued INTEGER NOT NULL DEFAULT 0,
				payload BLOB NOT NULL,
				UNIQUE (monitor_id, revision, observer_node_id, slot_unix_seconds, ad_hoc)
			);
			",
        )
        .unwrap();
    drop(connection);

    let handle = UptimeHandle::load(temporary.path()).unwrap();
    let mut first = observation(60);
    first.ad_hoc = true;
    let second = first.clone();
    assert!(
        handle
            .record_with_id("first-run".to_owned(), first)
            .await
            .unwrap()
    );
    assert!(
        handle
            .record_with_id("second-run".to_owned(), second)
            .await
            .unwrap()
    );
    assert_eq!(
        handle
            .observations("monitor", 0, 120, 10)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn applies_ad_hoc_limit_per_token_fingerprint() {
    let temporary = TempDir::new().unwrap();
    let handle = UptimeHandle::load(temporary.path()).unwrap();
    for _ in 0..AD_HOC_RUNS_PER_MINUTE {
        drop(handle.acquire_ad_hoc(60, "token-a").await.unwrap());
    }
    assert!(handle.acquire_ad_hoc(60, "token-a").await.is_none());
    assert!(handle.acquire_ad_hoc(60, "token-b").await.is_some());
}

#[test]
fn scheduled_monitor_requires_exact_slot_and_active_lifecycle() {
    let monitor = ServiceMonitor {
        monitor_id: "monitor".to_owned(),
        name: "Example".to_owned(),
        target: MonitorTarget::Tcping {
            host: "example.com".to_owned(),
            port: 443,
        },
        interval_seconds: 60,
        observer_node_ids: Some(vec!["node".to_owned()]),
        lifecycle: MonitorLifecycle::Active,
        revision: 1,
        revision_effective_at_unix_seconds: 60,
    };
    assert!(scheduler::is_scheduled_on_local_node(&monitor, "node", 120));
    assert!(!scheduler::is_scheduled_on_local_node(
        &monitor, "node", 121
    ));
    assert!(!scheduler::is_scheduled_on_local_node(
        &monitor, "other", 120
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn validates_icmp_echo_reply_sequence() {
    assert!(is_icmp_echo_reply(&[0, 0, 0, 0, 0, 0, 0, 2], 2));
    assert!(!is_icmp_echo_reply(&[0, 0, 0, 0, 0, 0, 0, 3], 2));
    assert!(!is_icmp_echo_reply(&[8, 0, 0, 0, 0, 0, 0, 2], 2));
}
