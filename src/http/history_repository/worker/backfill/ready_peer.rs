use super::*;
use crate::state::history_repository::replica::{
    InitialPeerBackfillCheckpoint, InitialPeerRetainedAnchorStream, InitialPeerTieredHandoff,
    RetainedAnchorCheckpointUpdate,
};

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
    !checkpoint.retained_anchor_repair_response_seen
        && !checkpoint.retained_anchor_streams.iter().any(|stream| {
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
    if peers.len() != ready_repository_ids.len() {
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

    let mut in_progress = false;
    for peer in peers
        .iter()
        .filter(|peer| peer.node_id != state.cluster.node_id)
    {
        match advance_ready_peer_catch_up_page(state, peer, &receiving_repository_ids, now).await? {
            InitialBackfillProgress::InProgress => in_progress = true,
            InitialBackfillProgress::Complete => {}
            InitialBackfillProgress::Unavailable => {
                return Ok(InitialBackfillProgress::Unavailable);
            }
        }
    }
    if in_progress {
        return Ok(InitialBackfillProgress::InProgress);
    }
    // Tiered rows overlap across ready repositories. Keep the prior single-authority rule while
    // still advancing every peer's signed summary one bounded page per worker tick.
    let Some(tiered_peer) = peers
        .iter()
        .find(|peer| peer.node_id != state.cluster.node_id)
    else {
        return Ok(InitialBackfillProgress::Unavailable);
    };
    let needs_reverification = {
        let runtime = state.repository_replica.lock().await;
        peers.iter().any(|peer| {
            peer.node_id != state.cluster.node_id
                && runtime
                    .initial_peer_backfill_checkpoint(&peer.node_id)
                    .is_some_and(|checkpoint| checkpoint.summary_requires_tiered_backfill)
        })
    };
    let tiered_progress =
        pull_peer_initial_history(state, tiered_peer, &receiving_repository_ids).await?;
    if needs_reverification && tiered_progress == InitialBackfillProgress::Complete {
        let mut runtime = state.repository_replica.lock().await;
        for peer in peers
            .iter()
            .filter(|peer| peer.node_id != state.cluster.node_id)
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
    let repair: RepositoryRepairBatch = repository_direct_request(
        state,
        peer,
        Method::POST,
        "/api/admin/_internal/history-repository/repair",
        body,
    )
    .await?;
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
    if !checkpoint.retained_anchor_repair_response_seen
        && checkpoint
            .retained_anchor_repair_response_id
            .as_ref()
            .is_some_and(|existing| existing != &response_id)
    {
        anyhow::bail!("retained anchor repair response changed before completion");
    }
    let first_repair_response = !checkpoint.retained_anchor_repair_response_seen;
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
    if repair.history_truncated && first_repair_response {
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
            && first_repair_response
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
    fn tiered_handoff_is_limited_to_the_first_unconsumed_repair_page_stream() {
        let handoff = InitialPeerTieredHandoff {
            source_node_id: "node-a".to_owned(),
            source_epoch: 7,
            stream: "runtime".to_owned(),
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
                stream: handoff.stream.clone(),
            });
        assert!(!can_schedule_tiered_handoff(&stream_consumed, &handoff));

        let later_page = InitialPeerBackfillCheckpoint {
            retained_anchor_repair_response_seen: true,
            ..InitialPeerBackfillCheckpoint::default()
        };
        assert!(!can_schedule_tiered_handoff(&later_page, &handoff));
    }
}
