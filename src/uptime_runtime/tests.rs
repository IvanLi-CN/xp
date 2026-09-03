use rusqlite::{Connection, params};
use tempfile::TempDir;

use super::*;
use crate::uptime_monitor::MonitorLifecycle;

fn observation(slot: u64) -> Observation {
    Observation {
        monitor_id: "monitor".to_owned(),
        revision: 1,
        observer_node_id: "node".to_owned(),
        observer_set_node_ids: vec!["node".to_owned()],
        expected_observer_count: 1,
        slot_unix_seconds: slot,
        observed_at_unix_seconds: slot,
        outcome: ObservationOutcome::Success,
        error: None,
        latency_ms: Some(xp_test_fixtures::number_value4()),
        status_code: Some(200),
        packet_loss_percent: 0,
        ad_hoc: false,
    }
}

#[test]
fn tcp_connect_timeout_never_exceeds_the_shared_total_deadline() {
    assert_eq!(
        tcp_connect_timeout(Duration::ZERO),
        Some(Duration::from_secs(u64::from(
            DEFAULT_CONNECT_TIMEOUT_SECONDS
        )))
    );
    assert_eq!(
        tcp_connect_timeout(Duration::from_secs(7)),
        Some(Duration::from_secs(3))
    );
    assert_eq!(
        tcp_connect_timeout(Duration::from_secs(u64::from(
            DEFAULT_TOTAL_TIMEOUT_SECONDS
        ))),
        None
    );
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
async fn coalesces_skipped_slots_into_a_persisted_capture_gap() {
    let temporary = TempDir::new().unwrap();
    let handle = UptimeHandle::load(temporary.path()).unwrap();
    let monitor = ServiceMonitor {
        monitor_id: "monitor".to_owned(),
        name: "Example".to_owned(),
        target: MonitorTarget::Tcping {
            host: xp_test_fixtures::primary_host().to_owned(),
            port: 443,
        },
        interval_seconds: 60,
        observer_policy: crate::uptime_monitor::ObserverPolicy {
            mode: crate::uptime_monitor::ObserverPolicyMode::Include,
            node_ids: vec!["node".to_owned()],
        },
        lifecycle: MonitorLifecycle::Active,
        revision: 1,
        revision_effective_at_unix_seconds: 60,
    };

    assert!(
        handle
            .record_capture_gap(&monitor, "node".to_owned(), vec!["node".to_owned()], 60)
            .await
            .unwrap()
    );
    assert!(
        handle
            .record_capture_gap(&monitor, "node".to_owned(), vec!["node".to_owned()], 120)
            .await
            .unwrap()
    );
    let pending = handle.pending_capture_gaps(10).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].range.start_slot_unix_seconds, 60);
    assert_eq!(pending[0].range.end_slot_unix_seconds, 120);

    handle
        .mark_capture_gaps_enqueued(&[pending[0].id.clone()])
        .await
        .unwrap();
    assert!(handle.pending_capture_gaps(10).await.unwrap().is_empty());
    assert_eq!(
        handle
            .capture_gaps("monitor", 0, 180, 10)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn suspends_capture_when_the_gap_backlog_reaches_its_high_watermark() {
    let temporary = TempDir::new().unwrap();
    let handle = UptimeHandle::load(temporary.path()).unwrap();
    {
        let runtime = handle.inner.lock().await;
        let transaction = runtime.connection.unchecked_transaction().unwrap();
        for index in 0..(MAX_PENDING_CAPTURE_GAPS * HIGH_WATERMARK_PERCENT / 100) {
            transaction
                .execute(
                    "INSERT INTO uptime_capture_gaps
                     (id, monitor_id, revision, observer_node_id, interval_seconds,
                      observer_set_node_ids_json, start_slot_unix_seconds, end_slot_unix_seconds)
                     VALUES (?1, ?2, 1, 'node', 60, ?3, 60, 60)",
                    params![
                        format!("gap-{index}"),
                        format!("monitor-{index}"),
                        br#"["node"]"#,
                    ],
                )
                .unwrap();
        }
        transaction.commit().unwrap();
    }

    assert!(handle.capture_state().await.unwrap().suspended);
    let monitor = ServiceMonitor {
        monitor_id: "monitor".to_owned(),
        name: "Example".to_owned(),
        target: MonitorTarget::Tcping {
            host: xp_test_fixtures::primary_host().to_owned(),
            port: 443,
        },
        interval_seconds: 60,
        observer_policy: crate::uptime_monitor::ObserverPolicy {
            mode: crate::uptime_monitor::ObserverPolicyMode::Include,
            node_ids: vec!["node".to_owned()],
        },
        lifecycle: MonitorLifecycle::Active,
        revision: 1,
        revision_effective_at_unix_seconds: 60,
    };
    assert!(
        !handle
            .record_capture_gap(&monitor, "node".to_owned(), vec!["node".to_owned()], 60)
            .await
            .unwrap()
    );
    let pending = handle
        .pending_capture_gaps(
            usize::try_from(
                MAX_PENDING_CAPTURE_GAPS * (HIGH_WATERMARK_PERCENT - LOW_WATERMARK_PERCENT) / 100,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    handle
        .mark_capture_gaps_enqueued(&pending.iter().map(|gap| gap.id.clone()).collect::<Vec<_>>())
        .await
        .unwrap();
    assert!(!handle.capture_state().await.unwrap().suspended);
    assert!(
        handle
            .record_capture_gap(&monitor, "node".to_owned(), vec!["node".to_owned()], 60)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn ad_hoc_run_persists_its_terminal_observation() {
    let temporary = TempDir::new().unwrap();
    let handle = UptimeHandle::load(temporary.path()).unwrap();
    let run = AdHocRun {
        run_id: xp_test_fixtures::primary_probe_run_id().to_owned(),
        monitor_id: "monitor".to_owned(),
        state: AdHocRunState::Queued,
        created_at_unix_seconds: 60,
        completed_at_unix_seconds: None,
        observation: None,
        reason: None,
    };
    handle.create_ad_hoc_run(&run).await.unwrap();
    handle.mark_ad_hoc_run_running(&run.run_id).await.unwrap();
    let mut result = observation(61);
    result.ad_hoc = true;
    assert!(
        handle
            .record_with_id(run.run_id.clone(), result.clone())
            .await
            .unwrap()
    );
    handle
        .complete_ad_hoc_run(&run.run_id, &result)
        .await
        .unwrap();

    let saved = handle.ad_hoc_run(&run.run_id).await.unwrap().unwrap();
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

#[tokio::test]
async fn draft_cluster_test_is_isolated_from_observation_capture_and_expires() {
    let temporary = TempDir::new().unwrap();
    let handle = UptimeHandle::load(temporary.path()).unwrap();
    let run = DraftClusterTest {
        run_id: xp_test_fixtures::primary_probe_run_id().to_owned(),
        target: MonitorTarget::Ping {
            host: xp_test_fixtures::primary_host().to_owned(),
        },
        observer_policy: crate::uptime_monitor::ObserverPolicy::default(),
        observer_node_ids: vec![xp_test_fixtures::primary_node_id().to_owned()],
        coordinator_node_id: xp_test_fixtures::secondary_node_id().to_owned(),
        state: DraftClusterTestState::Queued,
        created_at_unix_seconds: 100,
        expires_at_unix_seconds: 200,
        observers: vec![DraftClusterTestObserver {
            node_id: xp_test_fixtures::primary_node_id().to_owned(),
            state: DraftClusterTestState::Queued,
            latency_ms: None,
            status_code: None,
            error: None,
            started_at_unix_seconds: None,
            completed_at_unix_seconds: None,
        }],
        reason: None,
    };
    handle.create_draft_test(&run).await.unwrap();
    handle
        .update_draft_test_observer(
            xp_test_fixtures::primary_probe_run_id(),
            DraftClusterTestObserverUpdate {
                node_id: xp_test_fixtures::primary_node_id().to_owned(),
                state: DraftClusterTestState::Succeeded,
                latency_ms: Some(xp_test_fixtures::number_value42()),
                status_code: Some(200),
                error: None,
                observed_at_unix_seconds: 120,
            },
        )
        .await
        .unwrap();
    let completed = handle
        .draft_test(xp_test_fixtures::primary_probe_run_id(), 120)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(completed.state, DraftClusterTestState::Succeeded));
    assert!(
        handle
            .observations(xp_test_fixtures::primary_probe_run_id(), 0, 300, 10)
            .await
            .unwrap()
            .is_empty()
    );
    let expired = handle
        .draft_test(xp_test_fixtures::primary_probe_run_id(), 200)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(expired.state, DraftClusterTestState::Interrupted));
}

#[tokio::test]
async fn draft_cluster_test_idempotency_reuses_matching_snapshot_and_rejects_conflicts() {
    let temporary = TempDir::new().unwrap();
    let handle = UptimeHandle::load(temporary.path()).unwrap();
    let run = DraftClusterTest {
        run_id: "draft-idempotent-1".to_owned(),
        target: MonitorTarget::Ping {
            host: xp_test_fixtures::primary_host().to_owned(),
        },
        observer_policy: crate::uptime_monitor::ObserverPolicy::default(),
        observer_node_ids: vec![],
        coordinator_node_id: xp_test_fixtures::primary_node_id().to_owned(),
        state: DraftClusterTestState::Queued,
        created_at_unix_seconds: 100,
        expires_at_unix_seconds: 1_000,
        observers: vec![],
        reason: None,
    };
    let first = handle
        .create_draft_test_idempotent(&run, "caller", Some("key"), "snapshot-a", 100)
        .await
        .unwrap();
    assert!(matches!(first, DraftTestCreateOutcome::Created(_)));

    let mut retry = run.clone();
    retry.run_id = "draft-idempotent-2".to_owned();
    let existing = handle
        .create_draft_test_idempotent(&retry, "caller", Some("key"), "snapshot-a", 101)
        .await
        .unwrap();
    assert!(matches!(
        existing,
        DraftTestCreateOutcome::Existing(run) if run.run_id == "draft-idempotent-1"
    ));

    let conflict = handle
        .create_draft_test_idempotent(&retry, "caller", Some("key"), "snapshot-b", 101)
        .await
        .unwrap();
    assert!(matches!(
        conflict,
        DraftTestCreateOutcome::IdempotencyConflict
    ));
}

#[test]
fn scheduled_monitor_requires_exact_slot_and_active_lifecycle() {
    let monitor = ServiceMonitor {
        monitor_id: "monitor".to_owned(),
        name: "Example".to_owned(),
        target: MonitorTarget::Tcping {
            host: xp_test_fixtures::primary_host().to_owned(),
            port: 443,
        },
        interval_seconds: 60,
        observer_policy: crate::uptime_monitor::ObserverPolicy {
            mode: crate::uptime_monitor::ObserverPolicyMode::Include,
            node_ids: vec!["node".to_owned()],
        },
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
