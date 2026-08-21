use super::*;
use crate::state::history_repository::replica::InitialPeerBackfillCheckpoint;

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
    let (requires_repair, missing_segment_ids) = {
        let runtime = state.repository_replica.lock().await;
        (
            runtime.requires_repair(&remote_summary, true)?,
            runtime.missing_segment_ids(&remote_summary, true)?,
        )
    };
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
                checkpoint.summary_requires_tiered_backfill,
            )?;
        return Ok(InitialBackfillProgress::InProgress);
    }
    let mut requires_tiered_backfill = checkpoint.summary_requires_tiered_backfill;
    if requires_repair {
        requires_tiered_backfill = true;
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
    })?;
    let repair: RepositoryRepairBatch = repository_direct_request(
        state,
        peer,
        Method::POST,
        "/api/admin/_internal/history-repository/repair",
        body,
    )
    .await?;
    let mut remaining = pending;
    super::super::remove_delivered_repair_segment_ids(
        &mut remaining,
        repair
            .segments
            .iter()
            .map(|segment| segment.wire.as_slice()),
    )?;
    for segment in repair.segments {
        if !super::super::super::identity_is_pinned_for_node(state, &segment.identity)
            .await
            .map_err(|_| anyhow::anyhow!("check repository repair segment identity"))?
        {
            anyhow::bail!("repository repair segment identity is not pinned");
        }
        state
            .repository_replica
            .lock()
            .await
            .receive_wire_from_repository(
                &state.cluster.cluster_id,
                &segment.identity,
                &segment.wire,
                now,
                ready_repository_ids,
                &state.cluster.node_id,
            )?;
    }
    state
        .repository_replica
        .lock()
        .await
        .merge_replica_gaps(&repair.gaps)?;
    let page_complete = remaining.is_empty();
    let summary_cursor = page_complete
        .then(|| checkpoint.summary_pending_next_cursor.clone())
        .flatten();
    let summary_complete = page_complete && summary_cursor.is_none();
    state
        .repository_replica
        .lock()
        .await
        .update_initial_peer_summary_checkpoint(
            &peer.node_id,
            summary_cursor,
            remaining.into_iter().collect(),
            if page_complete {
                None
            } else {
                checkpoint.summary_pending_next_cursor
            },
            summary_complete,
            checkpoint.summary_requires_tiered_backfill,
        )?;
    Ok(InitialBackfillProgress::InProgress)
}
