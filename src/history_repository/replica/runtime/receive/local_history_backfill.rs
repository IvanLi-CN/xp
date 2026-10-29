use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn receive_local_history_backfill_page(
    runtime: &mut RepositoryReplicaRuntime,
    cluster_id: &str,
    source_identity: &RepositoryNodeIdentity,
    segments: &[RepositoryReplicaSegment],
    backfill_epoch: u64,
    previous_stream_state: LocalHistoryBackfillStreamState,
    stream_state: LocalHistoryBackfillStreamState,
    page_cursor: Option<String>,
    completed: bool,
    now_unix_seconds: u64,
    ready_repositories: &[String],
    local_repository_id: &str,
) -> Result<(), RepositoryRuntimeError> {
    if runtime.snapshot.local_history_backfill_inflight.is_some() {
        return Err(RepositoryRuntimeError::Storage(
            "legacy local history backfill page is still in flight".to_owned(),
        ));
    }
    runtime.rebuild_if_stale(now_unix_seconds)?;
    runtime.refresh_capacity()?;
    let availability = runtime.snapshot.capacity.history_write_availability();
    if !availability.allows_history_writes() {
        return Err(RepositoryRuntimeError::WriteStopped(availability));
    }
    runtime.bind_cluster(cluster_id)?;
    runtime.ensure_receiver()?;
    let previous_snapshot = runtime.snapshot.clone();
    let previous_tombstones = runtime.tombstones.checkpoint();
    let previous_receiver = runtime
        .receiver
        .as_ref()
        .expect("receiver initialized")
        .checkpoint()?;
    let mut observed_stream_state = previous_stream_state.clone();
    let stream_suffix = format!("-backfill-{}", source_identity.node_id().as_str());
    let mut validate_segment = |segment: &RepositoryReplicaSegment| {
        if segment.identity != *source_identity {
            return Err(RepositoryRuntimeError::Protocol(
                ProtocolError::SourceIdentityMismatch,
            ));
        }
        let signed = SignedSegment::from_wire(&segment.wire)?;
        signed.verify_identity(source_identity)?;
        let first = signed.canonical().first_cursor();
        if first.source_epoch() != backfill_epoch
            || first.source_node_id() != source_identity.node_id().as_str()
            || !first.stream().ends_with(&stream_suffix)
        {
            return Err(RepositoryRuntimeError::Protocol(
                ProtocolError::CursorIdentityMismatch,
            ));
        }
        let expected_sequence = observed_stream_state
            .get(first.stream())
            .map_or(0, |(sequence, _)| *sequence);
        if first.sequence() != expected_sequence {
            return Err(RepositoryRuntimeError::Protocol(
                ProtocolError::SequenceGap {
                    expected: expected_sequence,
                    actual: first.sequence(),
                },
            ));
        }
        let next_sequence = signed
            .canonical()
            .last_cursor()
            .sequence()
            .checked_add(1)
            .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
        observed_stream_state.insert(
            first.stream().to_owned(),
            (next_sequence, Some(signed.segment_hash()?)),
        );
        Ok(())
    };
    for segment in segments {
        if let Err(error) = validate_segment(segment) {
            runtime.restore(&previous_receiver, previous_snapshot.clone())?;
            runtime.tombstones = TombstoneLedger::from_checkpoint(previous_tombstones.clone())?;
            return Err(error);
        }
    }
    if observed_stream_state != stream_state {
        runtime.restore(&previous_receiver, previous_snapshot.clone())?;
        runtime.tombstones = TombstoneLedger::from_checkpoint(previous_tombstones)?;
        return Err(RepositoryRuntimeError::Storage(
            "local history backfill stream checkpoint does not match its page".to_owned(),
        ));
    }
    let mut mutation = PendingRepositoryMutation {
        target_repository_id: local_repository_id.to_owned(),
        progress_at_unix_seconds: Some(now_unix_seconds),
        ..PendingRepositoryMutation::default()
    };
    for segment in segments {
        if let Err(error) = runtime.receive_wire_from_repository_inner(
            RepositoryWireRequest {
                cluster_id,
                identity: &segment.identity,
                wire: &segment.wire,
                now_unix_seconds,
                ready_repositories,
                local_repository_id,
                allow_retained_anchor: false,
                rollback_snapshot: Some(previous_snapshot.clone()),
                discard_gap_evidence_on_error: true,
                allow_retained_sequence_gap: false,
                retained_anchor_checkpoint: None,
            },
            Some(&mut mutation),
        ) {
            runtime.restore(&previous_receiver, previous_snapshot.clone())?;
            runtime.tombstones = TombstoneLedger::from_checkpoint(previous_tombstones.clone())?;
            return Err(error);
        }
    }
    runtime.snapshot.local_history_backfill_epoch = backfill_epoch;
    runtime.snapshot.local_history_backfill_stream_state = stream_state;
    runtime.snapshot.local_history_backfill_cursor = page_cursor;
    runtime.snapshot.local_history_backfill_completed = completed;
    runtime.persist_or_restore_with_mutation(&previous_receiver, &previous_snapshot, mutation)?;
    if let Err(error) = runtime.prune_retention(now_unix_seconds) {
        runtime.storage_degraded = true;
        tracing::warn!(
            error = %error,
            "history retention maintenance deferred after local backfill receive"
        );
    }
    Ok(())
}
