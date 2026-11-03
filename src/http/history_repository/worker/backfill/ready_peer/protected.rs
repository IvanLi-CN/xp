use super::*;

pub(super) fn completed_repair_response(
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
        response_complete: true,
        allowance_complete: page_complete,
    }
}

pub(super) fn wire_is_protected(state: &AppState, wire: &[u8]) -> anyhow::Result<bool> {
    let signed = crate::history_sync::SignedSegment::from_wire(wire)?;
    let cursor = signed.canonical().first_cursor();
    Ok(tombstones::protected_streams(state)?
        .iter()
        .any(|stream| stream.source == cursor.source_node_id() && stream.stream == cursor.stream()))
}

pub(in crate::http::history_repository::worker::backfill) fn filter_gaps(
    state: &AppState,
    gaps: &mut Vec<crate::state::history_repository::replica::RepositoryReplicaGap>,
) -> anyhow::Result<bool> {
    let protected = tombstones::protected_streams(state)?;
    let mut removed = false;
    for gap in gaps.iter() {
        if protected
            .iter()
            .any(|stream| stream.source == gap.source_node_id && stream.stream == gap.stream)
        {
            anyhow::ensure!(
                protected.iter().any(|stream| {
                    stream.source == gap.source_node_id
                        && stream.stream == gap.stream
                        && stream.epoch == gap.source_epoch
                        && gap.first_sequence <= gap.last_sequence
                        && stream.last.is_some_and(|last| gap.last_sequence <= last)
                }),
                "peer permanent gap exceeds the verified protected prefix"
            );
        }
    }
    gaps.retain(|gap| {
        let covered = protected
            .iter()
            .any(|stream| stream.source == gap.source_node_id && stream.stream == gap.stream);
        removed |= covered;
        !covered
    });
    Ok(removed)
}

pub(super) async fn refreshed_filter_gaps(
    state: &AppState,
    gaps: &mut Vec<crate::state::history_repository::replica::RepositoryReplicaGap>,
) -> anyhow::Result<bool> {
    tombstones::refresh(state).await?;
    filter_gaps(state, gaps)
}

pub(in crate::http::history_repository::worker::backfill) async fn receive(
    state: &AppState,
    segment: &crate::state::history_repository::replica::RepositoryReplicaSegment,
    gaps: &[crate::state::history_repository::replica::RepositoryReplicaGap],
    now: u64,
    ready: &[String],
) -> anyhow::Result<bool> {
    tombstones::refresh(state).await?;
    anyhow::ensure!(
        super::super::super::super::identity_is_valid_for_history_replay(state, &segment.identity)
            .await
            .map_err(|_| anyhow::anyhow!("check protected repair identity"))?,
        "protected repair identity is not pinned"
    );
    let signed = crate::history_sync::SignedSegment::from_wire(&segment.wire)?;
    signed.verify_identity(&segment.identity)?;
    let mut runtime = state.repository_replica.lock().await;
    // Do not let retained-anchor acceptance turn recoverable local predecessors into gaps.
    if runtime
        .tiered_handoff_for_sequence_gap(&segment.wire)?
        .is_some()
    {
        return Ok(false);
    }
    runtime.receive_repair_wire_from_repository_with_gaps(
        &state.cluster.cluster_id,
        &segment.identity,
        &segment.wire,
        gaps,
        now,
        ready,
        &state.cluster.node_id,
    )?;
    drop(runtime);
    tombstones::extend_verified(state, signed.canonical().last_cursor()).await?;
    Ok(true)
}
