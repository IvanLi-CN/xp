use super::*;
use crate::state::history_repository::replica::{
    InitialPeerBackfillCheckpoint, InitialPeerRetainedAnchorStream, InitialPeerTieredHandoff,
    RetainedAnchorCheckpointUpdate,
};
use std::{future::Future, time::Instant};

const MAX_INITIAL_CATCH_UP_PAGES_PER_TICK: usize = 8;
const INITIAL_CATCH_UP_TICK_BUDGET: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CatchUpDrain {
    progress: InitialBackfillProgress,
    pages_consumed: usize,
}

struct CompletedRepairResponse {
    summary_cursor: Option<String>,
    pending_segment_ids: Vec<String>,
    pending_next_cursor: Option<String>,
    summary_complete: bool,
    response_complete: bool,
    allowance_complete: bool,
}

fn completed_repair_response(
    checkpoint: &InitialPeerBackfillCheckpoint,
    remaining: BTreeSet<String>,
) -> CompletedRepairResponse {
    let page_complete = remaining.is_empty();
    let summary_cursor = page_complete
        .then(|| checkpoint.summary_pending_next_cursor.clone())
        .flatten();
    let summary_complete = page_complete && summary_cursor.is_none();
    CompletedRepairResponse {
        summary_cursor,
        pending_segment_ids: remaining.into_iter().collect(),
        pending_next_cursor: (!page_complete)
            .then(|| checkpoint.summary_pending_next_cursor.clone())
            .flatten(),
        summary_complete,
        // The wire-bounded response has been consumed even if it left IDs for
        // the next request, whose content has a distinct response identity.
        response_complete: true,
        allowance_complete: page_complete,
    }
}

fn can_schedule_tiered_handoff(
    checkpoint: &InitialPeerBackfillCheckpoint,
    handoff: &InitialPeerTieredHandoff,
) -> bool {
    !checkpoint.retained_anchor_streams.iter().any(|stream| {
        stream.source_node_id == handoff.source_node_id
            && stream.source_epoch == handoff.source_epoch
            && stream.stream == handoff.stream
    })
}

pub(crate) async fn catch_up_against_ready_repositories(
    state: &AppState,
    now: u64,
) -> anyhow::Result<InitialBackfillProgress> {
    let (ready_repository_ids, peers) = ready_repository_peers(state).await?;
    if peers.is_empty() {
        return Ok(InitialBackfillProgress::Unavailable);
    }
    let mut receiving_repository_ids = ready_repository_ids.clone();
    receiving_repository_ids.push(state.cluster.node_id.clone());
    receiving_repository_ids.sort_unstable();
    receiving_repository_ids.dedup();
    {
        let mut runtime = state.repository_replica.lock().await;
        runtime.prepare_for_replication(now)?;
        runtime.reconcile_ready_repositories(&receiving_repository_ids)?;
    }

    let deadline = Instant::now() + INITIAL_CATCH_UP_TICK_BUDGET;
    let peer_targets = peers
        .iter()
        .filter(|peer| peer.node_id != state.cluster.node_id)
        .collect::<Vec<_>>();
    let (peer_progress, pages_by_peer, progress_by_peer) =
        drain_ready_peer_pages(peer_targets.len(), deadline, |index| {
            let peer = peer_targets[index];
            async {
                advance_ready_peer_catch_up_page(state, peer, &receiving_repository_ids, now).await
            }
        })
        .await?;
    match peer_progress {
        InitialBackfillProgress::InProgress => return Ok(InitialBackfillProgress::InProgress),
        InitialBackfillProgress::Complete => {}
        InitialBackfillProgress::Unavailable => return Ok(InitialBackfillProgress::Unavailable),
    }
    // Tiered rows overlap across ready repositories. Keep the prior single-authority rule while
    // still advancing every peer's signed summary through bounded pages per worker tick.
    let Some((tiered_peer_index, tiered_peer)) = peer_targets
        .iter()
        .zip(progress_by_peer.iter())
        .enumerate()
        .find_map(|(index, (peer, progress))| {
            (*progress != InitialBackfillProgress::Unavailable).then_some((index, *peer))
        })
    else {
        return Ok(InitialBackfillProgress::Unavailable);
    };
    let needs_reverification = {
        let runtime = state.repository_replica.lock().await;
        peer_targets
            .iter()
            .zip(progress_by_peer.iter())
            .any(|(peer, progress)| {
                *progress != InitialBackfillProgress::Unavailable
                    && runtime
                        .initial_peer_backfill_checkpoint(&peer.node_id)
                        .is_some_and(|checkpoint| checkpoint.summary_requires_tiered_backfill)
            })
    };
    let tiered_page_cap = pages_by_peer
        .get(tiered_peer_index)
        .copied()
        .map_or(0, remaining_peer_page_budget);
    let tiered_progress = if tiered_page_cap == 0 {
        InitialBackfillProgress::InProgress
    } else {
        drain_bounded_catch_up_pages(tiered_page_cap, deadline, || async {
            pull_peer_initial_history(state, tiered_peer, &receiving_repository_ids).await
        })
        .await?
        .progress
    };
    if needs_reverification && tiered_progress == InitialBackfillProgress::Complete {
        let mut runtime = state.repository_replica.lock().await;
        for peer in peer_targets
            .iter()
            .zip(progress_by_peer.iter())
            .filter(|(_, progress)| **progress != InitialBackfillProgress::Unavailable)
            .map(|(peer, _)| *peer)
        {
            runtime.update_initial_peer_summary_checkpoint(
                &peer.node_id,
                None,
                Vec::new(),
                None,
                false,
                false,
            )?;
        }
        return Ok(InitialBackfillProgress::InProgress);
    }
    Ok(tiered_progress)
}

fn remaining_peer_page_budget(pages_consumed: usize) -> usize {
    MAX_INITIAL_CATCH_UP_PAGES_PER_TICK.saturating_sub(pages_consumed)
}

async fn drain_ready_peer_pages<F, Fut>(
    peer_count: usize,
    deadline: Instant,
    mut fetch_page: F,
) -> anyhow::Result<(
    InitialBackfillProgress,
    Vec<usize>,
    Vec<InitialBackfillProgress>,
)>
where
    F: FnMut(usize) -> Fut,
    Fut: Future<Output = anyhow::Result<InitialBackfillProgress>>,
{
    let mut in_progress = false;
    let mut unavailable = false;
    let mut complete = false;
    let mut pages_by_peer = Vec::with_capacity(peer_count);
    let mut progress_by_peer = Vec::with_capacity(peer_count);
    for index in 0..peer_count {
        let remaining_budget = deadline.saturating_duration_since(Instant::now());
        if remaining_budget.is_zero() {
            in_progress = true;
            break;
        }
        let remaining_peers = (peer_count - index) as u32;
        let peer_deadline = Instant::now() + remaining_budget / remaining_peers;
        let drain = drain_bounded_catch_up_pages(
            MAX_INITIAL_CATCH_UP_PAGES_PER_TICK,
            peer_deadline,
            || fetch_page(index),
        )
        .await?;
        pages_by_peer.push(drain.pages_consumed);
        progress_by_peer.push(drain.progress);
        match drain.progress {
            InitialBackfillProgress::InProgress => in_progress = true,
            InitialBackfillProgress::Complete => complete = true,
            InitialBackfillProgress::Unavailable => {
                // A failed public peer must not consume the whole tick. Continue with the
                // remaining peers, then report the aggregate result for the next retry.
                unavailable = true;
            }
        }
    }
    Ok((
        if in_progress {
            InitialBackfillProgress::InProgress
        } else if complete {
            InitialBackfillProgress::Complete
        } else if unavailable {
            InitialBackfillProgress::Unavailable
        } else {
            InitialBackfillProgress::Complete
        },
        pages_by_peer,
        progress_by_peer,
    ))
}

async fn drain_bounded_catch_up_pages<F, Fut>(
    max_pages: usize,
    deadline: Instant,
    mut fetch_page: F,
) -> anyhow::Result<CatchUpDrain>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = anyhow::Result<InitialBackfillProgress>>,
{
    let mut pages_consumed = 0;
    for _ in 0..max_pages {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(CatchUpDrain {
                progress: InitialBackfillProgress::InProgress,
                pages_consumed,
            });
        }
        pages_consumed += 1;
        let progress = match tokio::time::timeout(remaining, fetch_page()).await {
            Ok(progress) => progress?,
            Err(_) => {
                return Ok(CatchUpDrain {
                    progress: InitialBackfillProgress::InProgress,
                    pages_consumed,
                });
            }
        };
        match progress {
            InitialBackfillProgress::Complete => {
                return Ok(CatchUpDrain {
                    progress,
                    pages_consumed,
                });
            }
            InitialBackfillProgress::Unavailable => {
                return Ok(CatchUpDrain {
                    progress,
                    pages_consumed,
                });
            }
            InitialBackfillProgress::InProgress => {}
        }
    }
    Ok(CatchUpDrain {
        progress: InitialBackfillProgress::InProgress,
        pages_consumed,
    })
}

async fn advance_ready_peer_catch_up_page(
    state: &AppState,
    peer: &MeshPeerTarget,
    ready_repository_ids: &[String],
    now: u64,
) -> anyhow::Result<InitialBackfillProgress> {
    let checkpoint = state
        .repository_replica
        .lock()
        .await
        .initial_peer_backfill_checkpoint(&peer.node_id)
        .unwrap_or_default();
    if let Some(handoff) = checkpoint.summary_tiered_handoff.clone() {
        let tiered_progress = pull_peer_initial_history(state, peer, ready_repository_ids).await?;
        if tiered_progress == InitialBackfillProgress::Complete {
            state
                .repository_replica
                .lock()
                .await
                .complete_initial_peer_tiered_handoff(&peer.node_id, &handoff)?;
            tracing::info!(
                peer = %peer.node_id,
                source = %handoff.source_node_id,
                stream = %handoff.stream,
                first_missing = handoff.first_missing,
                last_missing = handoff.last_missing,
                "history repair tiered handoff completed"
            );
            return Ok(InitialBackfillProgress::InProgress);
        }
        return Ok(tiered_progress);
    }
    if !checkpoint.summary_pending_segment_ids.is_empty() {
        return repair_ready_peer_catch_up_page(state, peer, ready_repository_ids, now, checkpoint)
            .await;
    }
    if checkpoint.summary_complete {
        return Ok(InitialBackfillProgress::Complete);
    }

    let path = checkpoint.summary_cursor.as_ref().map_or_else(
        || "/api/admin/_internal/history-repository/summary?deep_verification=true".to_owned(),
        |cursor| {
            format!("/api/admin/_internal/history-repository/summary?after_segment_id={cursor}")
        },
    );
    let remote_summary: RepositoryReplicaSummary =
        match repository_direct_request(state, peer, Method::GET, &path, Vec::new()).await {
            Ok(summary) => summary,
            Err(error) => {
                tracing::debug!(
                    peer = %peer.node_id,
                    error = %error,
                    "history repository bounded catch-up page failed"
                );
                return Ok(InitialBackfillProgress::Unavailable);
            }
        };
    let (requires_repair, missing_segment_ids, partitions_converged) = {
        let mut runtime = state.repository_replica.lock().await;
        (
            runtime.requires_repair(&remote_summary, true)?,
            runtime.missing_segment_ids(&remote_summary, true)?,
            runtime.retained_partitions_converged(&remote_summary)?,
        )
    };
    let requires_tiered_backfill =
        checkpoint.summary_requires_tiered_backfill || !partitions_converged;
    if requires_repair && !missing_segment_ids.is_empty() {
        state
            .repository_replica
            .lock()
            .await
            .update_initial_peer_summary_checkpoint(
                &peer.node_id,
                checkpoint.summary_cursor,
                missing_segment_ids,
                remote_summary.next_segment_id,
                false,
                requires_tiered_backfill,
            )?;
        return Ok(InitialBackfillProgress::InProgress);
    }
    if requires_repair {
        state
            .repository_replica
            .lock()
            .await
            .merge_replica_gaps(&remote_summary.gaps)?;
    }
    let summary_complete = remote_summary.next_segment_id.is_none();
    state
        .repository_replica
        .lock()
        .await
        .update_initial_peer_summary_checkpoint(
            &peer.node_id,
            remote_summary.next_segment_id,
            Vec::new(),
            None,
            summary_complete,
            requires_tiered_backfill,
        )?;
    Ok(InitialBackfillProgress::InProgress)
}

async fn repair_ready_peer_catch_up_page(
    state: &AppState,
    peer: &MeshPeerTarget,
    ready_repository_ids: &[String],
    now: u64,
    checkpoint: InitialPeerBackfillCheckpoint,
) -> anyhow::Result<InitialBackfillProgress> {
    let pending = checkpoint
        .summary_pending_segment_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let body = serde_json::to_vec(&RepositoryRepairRequest {
        segment_ids: pending.iter().cloned().collect(),
        response_id: checkpoint.retained_anchor_repair_response_id.clone(),
    })?;
    let repair: RepositoryRepairBatch = match repository_direct_request(
        state,
        peer,
        Method::POST,
        "/api/admin/_internal/history-repository/repair",
        body,
    )
    .await
    {
        Ok(repair) => repair,
        Err(error) => {
            tracing::debug!(
                peer = %peer.node_id,
                error = %error,
                "history repository repair page failed"
            );
            return Ok(InitialBackfillProgress::Unavailable);
        }
    };
    // Old peers omit response_id. Derive it from their actual response rather than the
    // request, so a changed retry cannot consume a first-response allowance.
    let response_id = repair.response_id_digest()?;
    if repair
        .response_id
        .as_ref()
        .is_some_and(|provided| provided != &response_id)
    {
        anyhow::bail!("repository repair response identity does not match its content");
    }
    if checkpoint
        .retained_anchor_repair_response_id
        .as_ref()
        .is_some_and(|existing| existing != &response_id)
    {
        anyhow::bail!("retained anchor repair response changed before completion");
    }
    let mut remaining = pending.clone();
    if repair.segments.is_empty() {
        if repair.unavailable_segment_ids.is_empty() {
            anyhow::bail!("repository repair response did not advance the requested segment set");
        }
    } else {
        super::super::remove_delivered_repair_segment_ids(
            &mut remaining,
            repair
                .segments
                .iter()
                .map(|segment| segment.wire.as_slice()),
        )?;
    }
    super::super::remove_unavailable_repair_segment_ids(
        &mut remaining,
        &repair.unavailable_segment_ids,
    )?;
    // A truncated repository can return a valid retained anchor (with no unavailable IDs) whose
    // first segment starts after this receiver's watermark. Detect that sequence gap before
    // applying any segment; otherwise the receive path rejects the anchor and retries forever.
    if repair.history_truncated {
        let tiered_handoff = {
            let runtime = state.repository_replica.lock().await;
            repair
                .segments
                .iter()
                .find_map(|segment| {
                    runtime
                        .tiered_handoff_for_sequence_gap(&segment.wire)
                        .transpose()
                })
                .transpose()?
        };
        if let Some(tiered_handoff) =
            tiered_handoff.filter(|handoff| can_schedule_tiered_handoff(&checkpoint, handoff))
        {
            // Do not remove any segment from the original request yet. The bounded response has
            // not been received while the predecessor gap is bridged; retrying the same request
            // keeps its response identity stable and lets the anchor be applied afterwards.
            let pending_segment_ids = pending.iter().cloned().collect();
            let repair_gaps = repair.gaps.clone();
            let mut retained_anchor_streams = checkpoint.retained_anchor_streams.clone();
            retained_anchor_streams.insert(InitialPeerRetainedAnchorStream {
                source_node_id: tiered_handoff.source_node_id.clone(),
                source_epoch: tiered_handoff.source_epoch,
                stream: tiered_handoff.stream.clone(),
            });
            let mut runtime = state.repository_replica.lock().await;
            runtime.merge_replica_gaps_in_memory(&repair_gaps)?;
            runtime.start_initial_peer_tiered_handoff(&peer.node_id, tiered_handoff)?;
            runtime.update_initial_peer_summary_checkpoint_with_retained_anchor_response(
                &peer.node_id,
                checkpoint.summary_cursor.clone(),
                pending_segment_ids,
                checkpoint.summary_pending_next_cursor.clone(),
                false,
                true,
                Some(response_id),
                false,
                false,
                retained_anchor_streams,
            )?;
            tracing::warn!(
                peer = %peer.node_id,
                "history repair page crossed retention boundary; tiered backfill scheduled"
            );
            return Ok(InitialBackfillProgress::InProgress);
        }
    }
    if repair.segments.is_empty() && !repair.gaps.is_empty() {
        state
            .repository_replica
            .lock()
            .await
            .merge_replica_gaps(&repair.gaps)?;
    }
    let repair_gaps = repair.gaps;
    let mut retained_anchor_streams = checkpoint.retained_anchor_streams.clone();
    for (index, segment) in repair.segments.into_iter().enumerate() {
        if !super::super::super::identity_is_valid_for_history_replay(state, &segment.identity)
            .await
            .map_err(|_| anyhow::anyhow!("check repository repair segment identity"))?
        {
            anyhow::bail!("repository repair segment identity is not pinned");
        }
        let first_cursor = crate::history_sync::SignedSegment::from_wire(&segment.wire)?
            .canonical()
            .first_cursor()
            .clone();
        let stream_key = (
            first_cursor.source_node_id().to_owned(),
            first_cursor.source_epoch(),
            first_cursor.stream().to_owned(),
        );
        let allow_retained_sequence_gap = if repair.history_truncated
            && !retained_anchor_streams.iter().any(|key| {
                key.source_node_id == stream_key.0
                    && key.source_epoch == stream_key.1
                    && key.stream == stream_key.2
            }) {
            let runtime = state.repository_replica.lock().await;
            runtime.can_accept_retained_sequence_gap(&segment.wire, false)?
        } else {
            false
        };
        if allow_retained_sequence_gap {
            retained_anchor_streams.insert(InitialPeerRetainedAnchorStream {
                source_node_id: stream_key.0,
                source_epoch: stream_key.1,
                stream: stream_key.2,
            });
        }
        state
            .repository_replica
            .lock()
            .await
            .receive_initial_backfill_wire_from_repository_with_gaps_and_retained_anchor_state(
                &state.cluster.cluster_id,
                &segment.identity,
                &segment.wire,
                if index == 0 { &repair_gaps } else { &[] },
                now,
                ready_repository_ids,
                &state.cluster.node_id,
                allow_retained_sequence_gap,
                Some(RetainedAnchorCheckpointUpdate {
                    peer_node_id: peer.node_id.clone(),
                    response_id: response_id.clone(),
                    response_complete: false,
                    allowance_complete: false,
                    streams: retained_anchor_streams.clone(),
                }),
            )?;
    }
    let completed_response = completed_repair_response(&checkpoint, remaining);
    state
        .repository_replica
        .lock()
        .await
        .update_initial_peer_summary_checkpoint_with_retained_anchor_response(
            &peer.node_id,
            completed_response.summary_cursor,
            completed_response.pending_segment_ids,
            completed_response.pending_next_cursor,
            completed_response.summary_complete,
            checkpoint.summary_requires_tiered_backfill,
            Some(response_id),
            completed_response.response_complete,
            completed_response.allowance_complete,
            retained_anchor_streams,
        )?;
    Ok(InitialBackfillProgress::InProgress)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bounded_catch_up_drain_consumes_multiple_pages_in_one_tick() {
        let mut calls = 0;
        let result =
            drain_bounded_catch_up_pages(8, Instant::now() + Duration::from_secs(1), || {
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

    #[tokio::test]
    async fn bounded_catch_up_drain_stops_at_page_cap() {
        let mut calls = 0;
        let result =
            drain_bounded_catch_up_pages(3, Instant::now() + Duration::from_secs(1), || {
                calls += 1;
                async { Ok(InitialBackfillProgress::InProgress) }
            })
            .await
            .expect("bounded catch-up drain");

        assert_eq!(calls, 3);
        assert_eq!(result.progress, InitialBackfillProgress::InProgress);
        assert_eq!(result.pages_consumed, 3);
    }

    #[tokio::test]
    async fn bounded_catch_up_drain_stops_before_an_expired_deadline() {
        let mut calls = 0;
        let result =
            drain_bounded_catch_up_pages(8, Instant::now() - Duration::from_secs(1), || {
                calls += 1;
                async { Ok(InitialBackfillProgress::InProgress) }
            })
            .await
            .expect("bounded catch-up drain");

        assert_eq!(calls, 0);
        assert_eq!(result.progress, InitialBackfillProgress::InProgress);
        assert_eq!(result.pages_consumed, 0);
    }

    #[tokio::test]
    async fn bounded_catch_up_drain_cancels_a_slow_page_at_the_deadline() {
        let mut calls = 0;
        let result = tokio::time::timeout(
            Duration::from_millis(200),
            drain_bounded_catch_up_pages(8, Instant::now() + Duration::from_millis(20), || {
                calls += 1;
                async {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    Ok(InitialBackfillProgress::InProgress)
                }
            }),
        )
        .await
        .expect("bounded catch-up drain must honor its deadline")
        .expect("bounded catch-up drain");

        assert_eq!(calls, 1);
        assert_eq!(result.progress, InitialBackfillProgress::InProgress);
        assert_eq!(result.pages_consumed, 1);
    }

    #[tokio::test]
    async fn bounded_catch_up_drain_stops_after_an_unavailable_page() {
        let mut calls = 0;
        let result =
            drain_bounded_catch_up_pages(8, Instant::now() + Duration::from_secs(1), || {
                calls += 1;
                async { Ok(InitialBackfillProgress::Unavailable) }
            })
            .await
            .expect("bounded catch-up drain");

        assert_eq!(calls, 1);
        assert_eq!(result.progress, InitialBackfillProgress::Unavailable);
        assert_eq!(result.pages_consumed, 1);
    }

    #[tokio::test]
    async fn ready_peer_drain_gives_later_peers_a_slice_after_a_slow_peer() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded_calls = calls.clone();
        let result = tokio::time::timeout(
            Duration::from_millis(200),
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
            ),
        )
        .await
        .expect("ready peer drain must remain bounded")
        .expect("ready peer drain");

        assert_eq!(*calls.lock().expect("read peer calls"), vec![0, 1]);
        assert_eq!(result.0, InitialBackfillProgress::InProgress);
        assert_eq!(result.1, vec![1, 1]);
    }

    #[tokio::test]
    async fn ready_peer_drain_continues_after_an_unavailable_peer() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded_calls = calls.clone();
        let result =
            drain_ready_peer_pages(2, Instant::now() + Duration::from_secs(1), move |index| {
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
        assert_eq!(result.0, InitialBackfillProgress::Complete);
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
        assert!(!can_schedule_tiered_handoff(&stream_consumed, &same_stream));

        let later_page = InitialPeerBackfillCheckpoint {
            retained_anchor_repair_response_seen: true,
            retained_anchor_streams: stream_consumed.retained_anchor_streams,
            ..InitialPeerBackfillCheckpoint::default()
        };
        assert!(can_schedule_tiered_handoff(&later_page, &handoff));
    }
}
