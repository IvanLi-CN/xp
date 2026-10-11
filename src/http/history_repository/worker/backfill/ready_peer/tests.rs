use super::*;

#[tokio::test(start_paused = true)]
async fn bounded_catch_up_drain_consumes_multiple_pages_in_one_tick() {
    let mut calls = 0;
    let result = drain_bounded_catch_up_pages(8, Instant::now() + Duration::from_secs(1), || {
        calls += 1;
        async move {
            Ok(if calls < 3 {
                InitialBackfillProgress::InProgress
            } else {
                InitialBackfillProgress::Complete
            })
        }
    })
    .await
    .expect("bounded catch-up drain");

    assert_eq!(calls, 3);
    assert_eq!(result.progress, InitialBackfillProgress::Complete);
    assert_eq!(result.pages_consumed, 3);
}

#[tokio::test(start_paused = true)]
async fn bounded_catch_up_drain_stops_at_page_cap() {
    let mut calls = 0;
    let result = drain_bounded_catch_up_pages(3, Instant::now() + Duration::from_secs(1), || {
        calls += 1;
        async { Ok(InitialBackfillProgress::InProgress) }
    })
    .await
    .expect("bounded catch-up drain");

    assert_eq!(calls, 3);
    assert_eq!(result.progress, InitialBackfillProgress::InProgress);
    assert_eq!(result.pages_consumed, 3);
}

#[tokio::test(start_paused = true)]
async fn bounded_catch_up_drain_stops_before_an_expired_deadline() {
    let mut calls = 0;
    let result = drain_bounded_catch_up_pages(8, Instant::now() - Duration::from_secs(1), || {
        calls += 1;
        async { Ok(InitialBackfillProgress::InProgress) }
    })
    .await
    .expect("bounded catch-up drain");

    assert_eq!(calls, 0);
    assert_eq!(result.progress, InitialBackfillProgress::InProgress);
    assert_eq!(result.pages_consumed, 0);
}

#[tokio::test(start_paused = true)]
async fn bounded_catch_up_drain_cancels_a_slow_page_at_the_deadline() {
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let recorded_calls = calls.clone();
    let task = tokio::spawn(async move {
        drain_bounded_catch_up_pages(8, Instant::now() + Duration::from_millis(20), || {
            recorded_calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            async {
                tokio::time::sleep(Duration::from_secs(1)).await;
                Ok(InitialBackfillProgress::InProgress)
            }
        })
        .await
    });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_millis(20)).await;
    let result = task
        .await
        .expect("bounded catch-up drain task")
        .expect("bounded catch-up drain");

    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    assert_eq!(result.progress, InitialBackfillProgress::InProgress);
    assert_eq!(result.pages_consumed, 1);
}

#[tokio::test(start_paused = true)]
async fn bounded_catch_up_drain_stops_after_an_unavailable_page() {
    let mut calls = 0;
    let result = drain_bounded_catch_up_pages(8, Instant::now() + Duration::from_secs(1), || {
        calls += 1;
        async { Ok(InitialBackfillProgress::Unavailable) }
    })
    .await
    .expect("bounded catch-up drain");

    assert_eq!(calls, 1);
    assert_eq!(result.progress, InitialBackfillProgress::Unavailable);
    assert_eq!(result.pages_consumed, 1);
}

#[tokio::test(start_paused = true)]
async fn ready_peer_drain_gives_later_peers_a_slice_after_a_slow_peer() {
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded_calls = calls.clone();
    let task = tokio::spawn(async move {
        drain_ready_peer_pages(
            2,
            Instant::now() + Duration::from_millis(40),
            move |index| {
                recorded_calls.lock().expect("record peer call").push(index);
                async move {
                    if index == 0 {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        Ok(InitialBackfillProgress::InProgress)
                    } else {
                        Ok(InitialBackfillProgress::Complete)
                    }
                }
            },
        )
        .await
    });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_millis(20)).await;
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_millis(20)).await;
    let result = task
        .await
        .expect("ready peer drain task")
        .expect("ready peer drain");

    assert_eq!(*calls.lock().expect("read peer calls"), vec![0, 1]);
    assert_eq!(result.0, InitialBackfillProgress::InProgress);
    assert_eq!(result.1, vec![1, 1]);
}

#[tokio::test(start_paused = true)]
async fn ready_peer_drain_continues_after_an_unavailable_peer() {
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded_calls = calls.clone();
    let result = drain_ready_peer_pages(2, Instant::now() + Duration::from_secs(1), move |index| {
        recorded_calls.lock().expect("record peer call").push(index);
        async move {
            if index == 0 {
                Ok(InitialBackfillProgress::Unavailable)
            } else {
                Ok(InitialBackfillProgress::Complete)
            }
        }
    })
    .await
    .expect("ready peer drain");

    assert_eq!(*calls.lock().expect("read peer calls"), vec![0, 1]);
    assert_eq!(result.0, InitialBackfillProgress::Unavailable);
    assert_eq!(result.1, vec![1, 1]);
    assert_eq!(
        result.2,
        vec![
            InitialBackfillProgress::Unavailable,
            InitialBackfillProgress::Complete,
        ]
    );
}
#[test]
fn tiered_export_uses_only_the_remaining_peer_page_budget() {
    assert_eq!(remaining_peer_page_budget(0), 8);
    assert_eq!(remaining_peer_page_budget(3), 5);
    assert_eq!(remaining_peer_page_budget(8), 0);
    assert_eq!(remaining_peer_page_budget(16), 0);
}

#[test]
fn unavailable_ready_peer_keeps_catch_up_incomplete_after_tiered_drain() {
    assert_eq!(
        ready_peer_catch_up_result(
            InitialBackfillProgress::Unavailable,
            false,
            InitialBackfillProgress::Complete,
        ),
        InitialBackfillProgress::Unavailable
    );
    assert_eq!(
        ready_peer_catch_up_result(
            InitialBackfillProgress::Complete,
            false,
            InitialBackfillProgress::Complete,
        ),
        InitialBackfillProgress::Complete
    );
}

#[test]
fn refreshed_first_summary_page_is_revisited_after_bounded_repair() {
    let checkpoint = InitialPeerBackfillCheckpoint {
        summary_pending_revisit_cursor: true,
        ..Default::default()
    };
    let response = completed_repair_response(&checkpoint, BTreeSet::new());
    assert!(response.summary_cursor.is_none());
    assert!(response.pending_segment_ids.is_empty());
    assert!(!response.summary_complete);
    assert!(response.response_complete);
}

#[test]
fn wire_bounded_repair_response_completes_its_identity_with_pending_ids() {
    let checkpoint = InitialPeerBackfillCheckpoint {
        summary_pending_next_cursor: Some("next-summary-page".to_owned()),
        ..InitialPeerBackfillCheckpoint::default()
    };
    let response = completed_repair_response(
        &checkpoint,
        BTreeSet::from(["remaining-segment".to_owned()]),
    );

    assert_eq!(
        response.pending_segment_ids,
        vec!["remaining-segment".to_owned()]
    );
    assert_eq!(
        response.pending_next_cursor.as_deref(),
        Some("next-summary-page")
    );
    assert!(response.summary_cursor.is_none());
    assert!(!response.summary_complete);
    assert!(response.response_complete);
    assert!(!response.allowance_complete);
}

#[test]
fn tiered_handoff_allows_an_unseen_stream_after_another_stream_consumed_allowance() {
    let handoff = InitialPeerTieredHandoff {
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "tombstone".to_owned(),
        first_missing: 1,
        last_missing: 2,
        next_sequence: 3,
        end_unix_seconds: 12,
    };
    assert!(can_schedule_tiered_handoff(
        &InitialPeerBackfillCheckpoint::default(),
        &handoff
    ));
    let mut stream_consumed = InitialPeerBackfillCheckpoint::default();
    stream_consumed
        .retained_anchor_streams
        .insert(InitialPeerRetainedAnchorStream {
            source_node_id: handoff.source_node_id.clone(),
            source_epoch: handoff.source_epoch,
            stream: "connections".to_owned(),
        });
    assert!(can_schedule_tiered_handoff(&stream_consumed, &handoff));
    let same_stream = InitialPeerTieredHandoff {
        stream: "connections".to_owned(),
        ..handoff.clone()
    };
    assert!(can_schedule_tiered_handoff(&stream_consumed, &same_stream));
    let active_handoff = InitialPeerBackfillCheckpoint {
        summary_tiered_handoff: Some(same_stream.clone()),
        retained_anchor_streams: stream_consumed.retained_anchor_streams.clone(),
        ..InitialPeerBackfillCheckpoint::default()
    };
    assert!(!can_schedule_tiered_handoff(&active_handoff, &same_stream));
    let completed_handoff = InitialPeerBackfillCheckpoint {
        retained_anchor_handoffs: BTreeSet::from([same_stream.clone()]),
        ..InitialPeerBackfillCheckpoint::default()
    };
    assert!(!can_schedule_tiered_handoff(
        &completed_handoff,
        &same_stream
    ));
    let armed_recovery = InitialPeerBackfillCheckpoint {
        recovery_generation: 1,
        recovery_generation_consumed: false,
        recovery_handoff: Some(same_stream.clone()),
        retained_anchor_handoffs: BTreeSet::from([same_stream.clone()]),
        ..InitialPeerBackfillCheckpoint::default()
    };
    let mut extended_recovery_handoff = same_stream.clone();
    extended_recovery_handoff.last_missing += 10;
    extended_recovery_handoff.next_sequence += 11;
    extended_recovery_handoff.end_unix_seconds += 1;
    assert!(can_schedule_tiered_handoff(
        &armed_recovery,
        &extended_recovery_handoff
    ));
    let consumed_recovery = InitialPeerBackfillCheckpoint {
        recovery_generation: 1,
        recovery_generation_consumed: true,
        retained_anchor_handoffs: BTreeSet::from([same_stream.clone()]),
        ..InitialPeerBackfillCheckpoint::default()
    };
    assert!(!can_schedule_tiered_handoff(
        &consumed_recovery,
        &same_stream
    ));
    assert!(!can_schedule_tiered_handoff(
        &consumed_recovery,
        &extended_recovery_handoff
    ));
}

#[test]
fn summary_repair_errors_have_stable_diagnostic_categories() {
    let cases = [
        (
            RepositoryDirectError::Transport(anyhow::anyhow!("connection reset")),
            ("transport_error", SummaryRepairResult::TransportError),
        ),
        (
            RepositoryDirectError::RepairResponseChanged,
            ("response_changed", SummaryRepairResult::ResponseChanged),
        ),
        (
            RepositoryDirectError::Application(anyhow::anyhow!(
                "repository repair response did not advance"
            )),
            ("no_progress", SummaryRepairResult::NoProgress),
        ),
        (
            RepositoryDirectError::Application(anyhow::anyhow!("sqlite storage failure")),
            ("storage_error", SummaryRepairResult::StorageError),
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(classify_summary_repair_error(&error), expected);
    }
    assert_eq!(
        serde_json::to_string(&SummaryRepairResult::ExplicitUnavailable)
            .expect("serialize explicit unavailable"),
        "\"explicit_unavailable\""
    );
}
