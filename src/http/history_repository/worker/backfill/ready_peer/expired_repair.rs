use super::*;

pub(super) async fn refresh(
    state: &AppState,
    peer: &MeshPeerTarget,
    checkpoint: &InitialPeerBackfillCheckpoint,
    repair: &RepositoryRepairBatch,
) -> anyhow::Result<InitialBackfillProgress> {
    let path = checkpoint.summary_cursor.as_ref().map_or_else(
        || "/api/admin/_internal/history-repository/summary".to_owned(),
        |cursor| {
            format!("/api/admin/_internal/history-repository/summary?after_segment_id={cursor}")
        },
    );
    let summary: RepositoryReplicaSummary =
        repository_direct_request(state, peer, Method::GET, &path, Vec::new()).await?;
    let mut runtime = state.repository_replica.lock().await;
    let fresh = runtime.missing_segment_ids(&summary, false)?;
    let pending = refreshed_pending(checkpoint, &repair.unavailable_segment_ids, &fresh);
    if pending == checkpoint.summary_pending_segment_ids {
        return Ok(InitialBackfillProgress::Unavailable);
    }
    runtime.refresh_initial_peer_recovery_pending(
        checkpoint,
        &peer.node_id,
        pending,
        [&repair.gaps, &summary.gaps],
        summary.next_segment_id,
    )?;
    tracing::warn!(peer = %peer.node_id, "expired recovery repair page refreshed without rearming");
    Ok(InitialBackfillProgress::InProgress)
}

fn refreshed_pending(
    checkpoint: &InitialPeerBackfillCheckpoint,
    unavailable: &[String],
    fresh: &[String],
) -> Vec<String> {
    let expired = unavailable.iter().collect::<BTreeSet<_>>();
    let mut pending = checkpoint
        .summary_pending_segment_ids
        .iter()
        .filter(|id| !expired.contains(id))
        .cloned()
        .collect::<Vec<_>>();
    for id in fresh {
        if pending.len() == 64 {
            break;
        }
        if !pending.contains(id) {
            pending.push(id.clone());
        }
    }
    pending
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_recovery_refresh_preserves_retained_ids_and_bounds_new_page() {
        let checkpoint = InitialPeerBackfillCheckpoint {
            summary_pending_segment_ids: (0..64).map(|id| format!("{id:064x}")).collect(),
            ..Default::default()
        };
        let expired = checkpoint.summary_pending_segment_ids[3..].to_vec();
        let fresh = (100..164)
            .map(|id| format!("{id:064x}"))
            .collect::<Vec<_>>();
        let pending = refreshed_pending(&checkpoint, &expired, &fresh);
        assert_eq!(pending.len(), 64);
        assert_eq!(&pending[..3], &checkpoint.summary_pending_segment_ids[..3]);
        assert_eq!(&pending[3..], &fresh[..61]);
    }
}
