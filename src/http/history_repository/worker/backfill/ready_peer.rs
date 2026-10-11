use super::*;
mod expired_repair;
mod handoff_selection;
mod summary_repair;
use crate::state::history_repository::replica::{
    InitialPeerBackfillCheckpoint, InitialPeerRetainedAnchorStream, InitialPeerTieredHandoff,
    RetainedAnchorCheckpointUpdate, SummaryRepairDiagnosticUpdate, SummaryRepairResult,
};
#[cfg(test)]
use handoff_selection::can_schedule_tiered_handoff;
use handoff_selection::select_tiered_handoff;
use std::future::Future;
use summary_repair::{
    classify_summary_repair_error, record_summary_repair_attempt, record_summary_repair_error,
    record_summary_repair_result, retry_after,
};
use tokio::time::Instant;

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

async fn record_summary_repair_internal_failure(
    state: &AppState,
    peer_node_id: &str,
    now: u64,
    message: &str,
) -> anyhow::Result<()> {
    let (error_code, result) = if message.contains("did not advance") {
        ("no_progress", SummaryRepairResult::NoProgress)
    } else {
        ("storage_error", SummaryRepairResult::StorageError)
    };
    record_summary_repair_result(
        state,
        peer_node_id,
        now,
        result,
        Some(error_code),
        Some(retry_after(now)),
    )
    .await
}

async fn request_repair_page(
    state: &AppState,
    peer: &MeshPeerTarget,
    pending: &BTreeSet<String>,
    response_id: Option<String>,
) -> Result<RepositoryRepairBatch, RepositoryDirectError> {
    let body = serde_json::to_vec(&RepositoryRepairRequest {
        segment_ids: pending.iter().cloned().collect(),
        response_id,
    })
    .map_err(|error| RepositoryDirectError::Application(error.into()))?;
    repository_direct_request(
        state,
        peer,
        Method::POST,
        "/api/admin/_internal/history-repository/repair",
        body,
    )
    .await
}

fn completed_repair_response(
    checkpoint: &InitialPeerBackfillCheckpoint,
    remaining: BTreeSet<String>,
) -> CompletedRepairResponse {
    let page_complete = remaining.is_empty();
    let summary_cursor = page_complete
        .then(|| checkpoint.summary_pending_next_cursor.clone())
        .flatten();
    let summary_complete =
        page_complete && summary_cursor.is_none() && !checkpoint.summary_pending_revisit_cursor;
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

pub(crate) async fn catch_up_against_ready_repositories(
    state: &AppState,
    now: u64,
) -> anyhow::Result<InitialBackfillProgress> {
    let (ready_repository_ids, peers, missing_metadata) =
        super::ready_repository_peers_for_catch_up(state).await?;
    if peers.is_empty() {
        return Ok(InitialBackfillProgress::Unavailable);
    }
    {
        let mut runtime = state.repository_replica.lock().await;
        runtime.prepare_for_replication(now)?;
        // Keep stale Ready members in the tombstone ledger even when their node metadata is
        // unavailable. Network fanout remains limited to `peers` below.
        runtime.reconcile_ready_repositories(&ready_repository_ids)?;
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
                advance_ready_peer_catch_up_page(state, peer, &ready_repository_ids, now).await
            }
        })
        .await?;
    match peer_progress {
        InitialBackfillProgress::InProgress => return Ok(InitialBackfillProgress::InProgress),
        InitialBackfillProgress::Complete => {}
        // An unavailable peer must not prevent a reachable peer from completing its
        // tiered handoff. The aggregate result remains unavailable below when no
        // reachable peer can make progress.
        InitialBackfillProgress::Unavailable => {}
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
            pull_peer_initial_history(state, tiered_peer, &ready_repository_ids).await
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
    Ok(ready_peer_catch_up_result(
        peer_progress,
        missing_metadata,
        tiered_progress,
    ))
}

fn remaining_peer_page_budget(pages_consumed: usize) -> usize {
    MAX_INITIAL_CATCH_UP_PAGES_PER_TICK.saturating_sub(pages_consumed)
}

fn ready_peer_catch_up_result(
    peer_progress: InitialBackfillProgress,
    missing_metadata: bool,
    tiered_progress: InitialBackfillProgress,
) -> InitialBackfillProgress {
    if peer_progress == InitialBackfillProgress::InProgress
        || tiered_progress == InitialBackfillProgress::InProgress
    {
        return InitialBackfillProgress::InProgress;
    }
    if missing_metadata
        || peer_progress == InitialBackfillProgress::Unavailable
        || tiered_progress == InitialBackfillProgress::Unavailable
    {
        return InitialBackfillProgress::Unavailable;
    }
    InitialBackfillProgress::Complete
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
            InitialBackfillProgress::Complete => {}
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
    if !checkpoint.summary_complete {
        record_summary_repair_attempt(state, &peer.node_id, now).await?;
    }
    if let Some(handoff) = checkpoint.summary_tiered_handoff.clone() {
        let tiered_progress = pull_peer_initial_history(state, peer, ready_repository_ids).await?;
        if tiered_progress == InitialBackfillProgress::Complete {
            state
                .repository_replica
                .lock()
                .await
                .complete_initial_peer_tiered_handoff_with_summary_repair_result(
                    &peer.node_id,
                    &handoff,
                    Some(SummaryRepairDiagnosticUpdate {
                        now_unix_seconds: now,
                        result: SummaryRepairResult::Progress,
                        error_code: None,
                        retry_after: None,
                    }),
                )?;
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
        if tiered_progress == InitialBackfillProgress::Unavailable {
            record_summary_repair_result(
                state,
                &peer.node_id,
                now,
                SummaryRepairResult::TransportError,
                Some("transport_error"),
                Some(retry_after(now)),
            )
            .await?;
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
        || {
            concat!(
                "/api/admin/_internal/history-repository/summary?",
                "summary_version=3&deep_verification=true"
            )
            .to_owned()
        },
        |cursor| {
            format!("/api/admin/_internal/history-repository/summary?after_segment_id={cursor}")
        },
    );
    let remote_summary: RepositoryReplicaSummary =
        match repository_direct_request(state, peer, Method::GET, &path, Vec::new()).await {
            Ok(summary) => summary,
            Err(error) if error.is_transport() => {
                record_summary_repair_result(
                    state,
                    &peer.node_id,
                    now,
                    SummaryRepairResult::TransportError,
                    Some("transport_error"),
                    Some(retry_after(now)),
                )
                .await?;
                tracing::debug!(
                    peer = %peer.node_id,
                    error = %error,
                    "history repository bounded catch-up page failed"
                );
                return Ok(InitialBackfillProgress::Unavailable);
            }
            Err(error) => {
                let (error_code, result) = classify_summary_repair_error(&error);
                record_summary_repair_result(
                    state,
                    &peer.node_id,
                    now,
                    result,
                    Some(error_code),
                    Some(retry_after(now)),
                )
                .await?;
                return Err(error.into());
            }
        };
    let (requires_repair, missing_segment_ids, partitions_converged) = {
        let runtime = state.repository_replica.lock().await;
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
            .persist_summary_repair_transition(
                &peer.node_id,
                SummaryRepairDiagnosticUpdate {
                    now_unix_seconds: now,
                    result: SummaryRepairResult::Progress,
                    error_code: None,
                    retry_after: None,
                },
                |runtime| {
                    if remote_summary.history_truncated {
                        runtime.mark_history_truncated_in_memory();
                    }
                    runtime.update_initial_peer_summary_checkpoint_in_memory(
                        &peer.node_id,
                        checkpoint.summary_cursor.clone(),
                        missing_segment_ids,
                        remote_summary.next_segment_id.clone(),
                        false,
                        requires_tiered_backfill,
                    );
                    Ok(())
                },
            )?;
        return Ok(InitialBackfillProgress::InProgress);
    }
    let summary_complete = remote_summary.next_segment_id.is_none();
    let summary_progressed = requires_repair
        || remote_summary.next_segment_id != checkpoint.summary_cursor
        || (summary_complete && !checkpoint.summary_complete);
    let summary_result = if summary_progressed {
        SummaryRepairResult::Progress
    } else {
        SummaryRepairResult::NoProgress
    };
    state
        .repository_replica
        .lock()
        .await
        .persist_summary_repair_transition(
            &peer.node_id,
            SummaryRepairDiagnosticUpdate {
                now_unix_seconds: now,
                result: summary_result,
                error_code: (!summary_progressed).then_some("no_progress".to_owned()),
                retry_after: (!summary_progressed).then(|| retry_after(now)),
            },
            |runtime| {
                if remote_summary.history_truncated {
                    runtime.mark_history_truncated_in_memory();
                }
                if requires_repair {
                    runtime.merge_replica_gaps_in_memory(&remote_summary.gaps)?;
                }
                runtime.update_initial_peer_summary_checkpoint_in_memory(
                    &peer.node_id,
                    remote_summary.next_segment_id.clone(),
                    Vec::new(),
                    None,
                    summary_complete,
                    requires_tiered_backfill,
                );
                Ok(())
            },
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
    let mut expected_response_id = checkpoint.retained_anchor_repair_response_id.clone();
    let mut repair: RepositoryRepairBatch =
        match request_repair_page(state, peer, &pending, expected_response_id.clone()).await {
            Ok(repair) => repair,
            Err(error)
                if checkpoint.retained_anchor_repair_response_id.is_some()
                    && error.is_repair_response_changed() =>
            {
                // The serving repository may legitimately change a still-pending bounded response
                // while this peer is offline. Drop only the stale response identity; the durable
                // segment set, summary cursor, gaps, and tiered handoff remain the source of truth.
                state
                    .repository_replica
                    .lock()
                    .await
                    .persist_summary_repair_transition(
                        &peer.node_id,
                        SummaryRepairDiagnosticUpdate {
                            now_unix_seconds: now,
                            result: SummaryRepairResult::ResponseChanged,
                            error_code: Some("response_changed".to_owned()),
                            retry_after: Some(retry_after(now)),
                        },
                        |runtime| {
                            runtime.clear_initial_peer_retained_anchor_response_id_in_memory(
                                &peer.node_id,
                            );
                            Ok(())
                        },
                    )?;
                expected_response_id = None;
                tracing::warn!(
                    peer = %peer.node_id,
                    "discarded stale repair response identity and retrying the pending page"
                );
                match request_repair_page(state, peer, &pending, None).await {
                    Ok(repair) => repair,
                    Err(error) if error.is_transport() => {
                        record_summary_repair_error(state, &peer.node_id, now, &error).await?;
                        tracing::debug!(
                            peer = %peer.node_id,
                            error = %error,
                            "history repository repair retry failed"
                        );
                        return Ok(InitialBackfillProgress::Unavailable);
                    }
                    Err(error) => {
                        record_summary_repair_error(state, &peer.node_id, now, &error).await?;
                        return Err(error.into());
                    }
                }
            }
            Err(error) => {
                if error.is_transport() {
                    record_summary_repair_error(state, &peer.node_id, now, &error).await?;
                    tracing::debug!(
                        peer = %peer.node_id,
                        error = %error,
                        "history repository repair page failed"
                    );
                    return Ok(InitialBackfillProgress::Unavailable);
                }
                record_summary_repair_error(state, &peer.node_id, now, &error).await?;
                return Err(error.into());
            }
        };
    // Old peers omit response_id. Derive it from their actual response rather than the
    // request, so a changed retry cannot consume a first-response allowance.
    let mut response_id = match repair.response_id_digest() {
        Ok(response_id) => response_id,
        Err(error) => {
            let message = error.to_string();
            record_summary_repair_internal_failure(state, &peer.node_id, now, &message).await?;
            return Err(error.into());
        }
    };
    if repair
        .response_id
        .as_ref()
        .is_some_and(|provided| provided != &response_id)
    {
        record_summary_repair_result(
            state,
            &peer.node_id,
            now,
            SummaryRepairResult::StorageError,
            Some("response_identity_mismatch"),
            Some(retry_after(now)),
        )
        .await?;
        anyhow::bail!("repository repair response identity does not match its content");
    }
    if expected_response_id
        .as_ref()
        .is_some_and(|expected| expected != &response_id)
    {
        // Older serving peers may return the changed page instead of rejecting the stale
        // response identity. Re-request the same durable pending IDs without that identity so
        // both sides agree on the new bounded response before applying it.
        state
            .repository_replica
            .lock()
            .await
            .persist_summary_repair_transition(
                &peer.node_id,
                SummaryRepairDiagnosticUpdate {
                    now_unix_seconds: now,
                    result: SummaryRepairResult::ResponseChanged,
                    error_code: Some("response_changed".to_owned()),
                    retry_after: Some(retry_after(now)),
                },
                |runtime| {
                    runtime.clear_initial_peer_retained_anchor_response_id_in_memory(&peer.node_id);
                    Ok(())
                },
            )?;
        tracing::warn!(
            peer = %peer.node_id,
            "repair response changed; retrying the pending page without the stale identity"
        );
        repair = match request_repair_page(state, peer, &pending, None).await {
            Ok(repair) => repair,
            Err(error) if error.is_transport() => {
                record_summary_repair_error(state, &peer.node_id, now, &error).await?;
                tracing::debug!(
                    peer = %peer.node_id,
                    error = %error,
                    "history repository repair retry failed"
                );
                return Ok(InitialBackfillProgress::Unavailable);
            }
            Err(error) => {
                record_summary_repair_error(state, &peer.node_id, now, &error).await?;
                return Err(error.into());
            }
        };
        response_id = match repair.response_id_digest() {
            Ok(response_id) => response_id,
            Err(error) => {
                let message = error.to_string();
                record_summary_repair_internal_failure(state, &peer.node_id, now, &message).await?;
                return Err(error.into());
            }
        };
        if repair
            .response_id
            .as_ref()
            .is_some_and(|provided| provided != &response_id)
        {
            record_summary_repair_result(
                state,
                &peer.node_id,
                now,
                SummaryRepairResult::StorageError,
                Some("response_identity_mismatch"),
                Some(retry_after(now)),
            )
            .await?;
            anyhow::bail!("repository repair response identity does not match its content");
        }
    }
    let mut remaining = pending.clone();
    if repair.segments.is_empty() {
        if repair.unavailable_segment_ids.is_empty() {
            record_summary_repair_result(
                state,
                &peer.node_id,
                now,
                SummaryRepairResult::NoProgress,
                Some("no_progress"),
                Some(retry_after(now)),
            )
            .await?;
            return Ok(InitialBackfillProgress::InProgress);
        }
    } else {
        if let Err(error) = super::super::remove_delivered_repair_segment_ids(
            &mut remaining,
            repair
                .segments
                .iter()
                .map(|segment| segment.wire.as_slice()),
        ) {
            let message = error.to_string();
            record_summary_repair_internal_failure(state, &peer.node_id, now, &message).await?;
            return Err(error);
        }
    }
    if let Err(error) = super::super::remove_unavailable_repair_segment_ids(
        &mut remaining,
        &repair.unavailable_segment_ids,
    ) {
        let message = error.to_string();
        record_summary_repair_internal_failure(state, &peer.node_id, now, &message).await?;
        return Err(error);
    }
    // A truncated repository can return a valid retained anchor (with no unavailable IDs) whose
    // first segment starts after this receiver's watermark. Detect that sequence gap before
    // applying any segment; otherwise the receive path rejects the anchor and retries forever.
    if repair.history_truncated {
        let tiered_handoff = {
            let runtime = state.repository_replica.lock().await;
            match select_tiered_handoff(
                &checkpoint,
                repair
                    .segments
                    .iter()
                    .map(|segment| runtime.tiered_handoff_for_sequence_gap(&segment.wire)),
            ) {
                Ok(tiered_handoff) => tiered_handoff,
                Err(error) => {
                    let message = error.to_string();
                    record_summary_repair_internal_failure(state, &peer.node_id, now, &message)
                        .await?;
                    return Err(error.into());
                }
            }
        };
        if let Some(tiered_handoff) = tiered_handoff {
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
            runtime.persist_summary_repair_transition(
                &peer.node_id,
                SummaryRepairDiagnosticUpdate {
                    now_unix_seconds: now,
                    result: SummaryRepairResult::Progress,
                    error_code: None,
                    retry_after: None,
                },
                |runtime| {
                    if repair.history_truncated {
                        runtime.mark_history_truncated_in_memory();
                    }
                    runtime.merge_replica_gaps_in_memory(&repair_gaps)?;
                    runtime.start_initial_peer_tiered_handoff_in_memory(
                        &peer.node_id,
                        tiered_handoff.clone(),
                    )?;
                    runtime.update_initial_peer_summary_checkpoint_with_anchor_in_memory(
                        &peer.node_id,
                        checkpoint.summary_cursor.clone(),
                        pending_segment_ids,
                        checkpoint.summary_pending_next_cursor.clone(),
                        false,
                        true,
                        Some(response_id.clone()),
                        false,
                        false,
                        retained_anchor_streams,
                    )?;
                    Ok(())
                },
            )?;
            tracing::warn!(
                peer = %peer.node_id,
                "history repair page crossed retention boundary; tiered backfill scheduled"
            );
            return Ok(InitialBackfillProgress::InProgress);
        }
        if checkpoint.recovery_generation > 0
            && !checkpoint.recovery_generation_consumed
            && !repair.unavailable_segment_ids.is_empty()
        {
            let progress = expired_repair::refresh(
                state,
                peer,
                &checkpoint,
                &repair,
                Some(SummaryRepairDiagnosticUpdate {
                    now_unix_seconds: now,
                    result: SummaryRepairResult::ExplicitUnavailable,
                    error_code: Some("source_retention_expired".to_owned()),
                    retry_after: None,
                }),
                repair.history_truncated,
            )
            .await?;
            return Ok(progress);
        }
    }
    let had_segments = !repair.segments.is_empty();
    let had_unavailable = !repair.unavailable_segment_ids.is_empty();
    let repair_gaps = repair.gaps;
    let mut retained_anchor_streams = checkpoint.retained_anchor_streams.clone();
    for (index, segment) in repair.segments.into_iter().enumerate() {
        let identity_valid = match super::super::super::identity_is_valid_for_history_replay(
            state,
            &segment.identity,
        )
        .await
        {
            Ok(identity_valid) => identity_valid,
            Err(error) => {
                record_summary_repair_internal_failure(
                    state,
                    &peer.node_id,
                    now,
                    "check repository repair segment identity",
                )
                .await?;
                return Err(anyhow::anyhow!(
                    "check repository repair segment identity: {error:?}"
                ));
            }
        };
        if !identity_valid {
            record_summary_repair_internal_failure(
                state,
                &peer.node_id,
                now,
                "repository repair segment identity is not pinned",
            )
            .await?;
            anyhow::bail!("repository repair segment identity is not pinned");
        }
        let first_cursor = match crate::history_sync::SignedSegment::from_wire(&segment.wire) {
            Ok(signed) => signed.canonical().first_cursor().clone(),
            Err(error) => {
                let message = error.to_string();
                record_summary_repair_internal_failure(state, &peer.node_id, now, &message).await?;
                return Err(error.into());
            }
        };
        let stream_key = (
            first_cursor.source_node_id().to_owned(),
            first_cursor.source_epoch(),
            first_cursor.stream().to_owned(),
        );
        let stream_has_retained_anchor = retained_anchor_streams.iter().any(|key| {
            key.source_node_id == stream_key.0
                && key.source_epoch == stream_key.1
                && key.stream == stream_key.2
        });
        let same_consumed_recovery_stream = checkpoint.recovery_generation_consumed
            && checkpoint.recovery_handoff.as_ref().is_some_and(|handoff| {
                handoff.source_node_id == stream_key.0
                    && handoff.source_epoch == stream_key.1
                    && handoff.stream == stream_key.2
            });
        let allow_retained_sequence_gap = if repair.history_truncated && !stream_has_retained_anchor
        {
            let runtime = state.repository_replica.lock().await;
            match runtime.can_accept_retained_sequence_gap(&segment.wire, false) {
                Ok(allowed) => allowed,
                Err(error) => {
                    let message = error.to_string();
                    drop(runtime);
                    record_summary_repair_internal_failure(state, &peer.node_id, now, &message)
                        .await?;
                    return Err(error.into());
                }
            }
        } else if repair.history_truncated
            && stream_has_retained_anchor
            && same_consumed_recovery_stream
        {
            let runtime = state.repository_replica.lock().await;
            match runtime.can_accept_consumed_retained_sequence_gap(&segment.wire) {
                Ok(allowed) => allowed,
                Err(error) => {
                    let message = error.to_string();
                    drop(runtime);
                    record_summary_repair_internal_failure(state, &peer.node_id, now, &message)
                        .await?;
                    return Err(error.into());
                }
            }
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
        let receive_result = state
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
            );
        if let Err(error) = receive_result {
            let message = error.to_string();
            record_summary_repair_internal_failure(state, &peer.node_id, now, &message).await?;
            return Err(error.into());
        }
    }
    let completed_response = completed_repair_response(&checkpoint, remaining);
    let result = if !had_segments && had_unavailable {
        SummaryRepairResult::ExplicitUnavailable
    } else {
        SummaryRepairResult::Progress
    };
    let error_code = (result == SummaryRepairResult::ExplicitUnavailable)
        .then_some("explicit_unavailable".to_owned());
    let mut runtime = state.repository_replica.lock().await;
    runtime.persist_summary_repair_transition(
        &peer.node_id,
        SummaryRepairDiagnosticUpdate {
            now_unix_seconds: now,
            result,
            error_code,
            retry_after: None,
        },
        |runtime| {
            if repair.history_truncated {
                runtime.mark_history_truncated_in_memory();
            }
            if !had_segments && !repair_gaps.is_empty() {
                runtime.merge_replica_gaps_in_memory(&repair_gaps)?;
            }
            runtime.update_initial_peer_summary_checkpoint_with_anchor_in_memory(
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
            Ok(())
        },
    )?;
    Ok(InitialBackfillProgress::InProgress)
}

#[cfg(test)]
#[path = "ready_peer/tests.rs"]
mod tests;
