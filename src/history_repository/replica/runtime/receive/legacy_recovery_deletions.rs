use super::*;

pub(super) fn requires_prepass(segment: &CanonicalSegment) -> bool {
    let tombstone_stream = segment.first_cursor().stream() == "tombstone";
    segment
        .records()
        .iter()
        .any(|record| record.is_tombstone() != tombstone_stream)
}

pub(super) fn ensure_same_segment_deletions(
    segment: &CanonicalSegment,
    mutation: &PendingRepositoryMutation,
) -> Result<(), RepositoryRuntimeError> {
    let first = segment.first_cursor();
    for (offset, record) in segment.records().iter().enumerate() {
        if record.is_tombstone() {
            continue;
        }
        let deleted = segment.records().iter().any(|candidate| {
            candidate.is_tombstone()
                && candidate.subject_node_id() == record.subject_node_id()
                && candidate.observer_node_id() == record.observer_node_id()
                && candidate.schema() == record.schema()
                && (candidate.record_key() == record.record_key()
                    || (candidate.record_key().ends_with(b":")
                        && record.record_key().starts_with(candidate.record_key())))
        });
        if !deleted {
            continue;
        }
        let sequence = first
            .sequence()
            .checked_add(
                u64::try_from(offset).map_err(|_| RepositoryRuntimeError::StateLimitExceeded)?,
            )
            .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
        if mutation.records.iter().any(|row| {
            !row.tombstone
                && row.source_node_id == first.source_node_id()
                && row.source_epoch == first.source_epoch()
                && row.stream == first.stream()
                && row.sequence == sequence
        }) {
            return Err(failure("legacy same-segment deletion was not suppressed"));
        }
    }
    Ok(())
}

fn ledger_hash(runtime: &RepositoryReplicaRuntime) -> Result<[u8; 32], RepositoryRuntimeError> {
    let bytes = serde_json::to_vec(&runtime.tombstones.checkpoint()).map_err(storage_error)?;
    Ok(Sha256::digest(bytes).into())
}

pub(super) fn ledger_matches(
    runtime: &RepositoryReplicaRuntime,
    state: &LegacyRecoveryState,
) -> Result<bool, RepositoryRuntimeError> {
    Ok(state.mixed_ledger_hash == Some(ledger_hash(runtime)?))
}

pub(super) fn upgrade_evidence_checkpoint(
    runtime: &mut RepositoryReplicaRuntime,
    state: &mut LegacyRecoveryState,
    initial: &LegacyRecoveryState,
) -> Result<(), RepositoryRuntimeError> {
    if state.mixed_evidence_version > 1 {
        return Err(failure("unsupported legacy deletion evidence checkpoint"));
    }
    if state.mixed_evidence_version == 0
        || (initial.mixed_deletions_required && !state.mixed_deletions_required)
    {
        let previous = state.clone();
        state.mixed_evidence_version = 1;
        state.mixed_deletions_required = true;
        state.mixed_deletions_done = false;
        state.mixed_deletions_position = 0;
        state.mixed_suffix_position = 0;
        state.mixed_suffix_count = 0;
        state.mixed_deletions_at = None;
        state.mixed_ledger_hash = None;
        if previous.mixed_evidence_version == 0 {
            state.suffix_deletions_unavailable = previous.targets.iter().any(|(stream, target)| {
                stream != "tombstone" && target.suffix_next > target.page_end
            });
        }
        runtime.legacy_recovery_save(Some(&previous), state, &[], &[], false)?;
    }
    Ok(())
}

pub(super) fn refresh_source_tails(
    runtime: &mut RepositoryReplicaRuntime,
    state: &mut LegacyRecoveryState,
    current: &LegacyRecoveryState,
) -> Result<(), RepositoryRuntimeError> {
    if !state.mixed_deletions_required || state.page_receipted {
        return Ok(());
    }
    let advanced = current
        .targets
        .iter()
        .filter_map(|(stream, target)| {
            let saved = state.targets.get(stream)?;
            (stream != "tombstone"
                && target.source_end > saved.source_end
                && saved.suffix_next == saved.source_end
                && saved.suffix_hash == saved.source_hash)
                .then_some((stream.clone(), target.source_end, target.source_hash))
        })
        .collect::<Vec<_>>();
    if !advanced.is_empty() {
        let previous = state.clone();
        for (stream, end, hash) in advanced {
            let target = state.targets.get_mut(&stream).expect("validated target");
            target.source_end = end;
            target.source_hash = hash;
        }
        runtime.legacy_recovery_save(Some(&previous), state, &[], &[], false)?;
    }
    Ok(())
}

pub(super) fn ensure_source_tails_current(
    state: &LegacyRecoveryState,
    current: &LegacyRecoveryState,
) -> Result<(), RepositoryRuntimeError> {
    if state.page_receipted {
        return Ok(());
    }
    if current.targets.iter().any(|(stream, target)| {
        stream != "tombstone"
            && state.targets.get(stream).is_some_and(|saved| {
                target.source_end != saved.source_end || target.source_hash != saved.source_hash
            })
    }) {
        return Err(failure(
            "legacy mixed source advanced; refresh recovery checkpoint",
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn apply_step(
    runtime: &mut RepositoryReplicaRuntime,
    cluster: &str,
    identity: &RepositoryNodeIdentity,
    expected: &LegacyRecoveryState,
    originals: &[SourceDeliveryJournalRow],
    now: u64,
    ready: &[String],
    local: &str,
) -> Result<bool, RepositoryRuntimeError> {
    page::validate_tombstone_wires(runtime, originals)?;
    if expected
        .mixed_deletions_at
        .is_some_and(|time| time > now || now - time >= TOMBSTONE_HORIZON_SECONDS)
        || (expected.mixed_deletions_at.is_some() && !ledger_matches(runtime, expected)?)
    {
        let mut next = expected.clone();
        next.mixed_deletions_position = 0;
        next.mixed_suffix_position = 0;
        next.mixed_suffix_count = 0;
        next.mixed_deletions_done = false;
        next.mixed_deletions_at = None;
        next.mixed_ledger_hash = None;
        runtime.legacy_recovery_save(Some(expected), &next, &[], &[], false)?;
        return Ok(false);
    }
    let staged = runtime
        .storage
        .legacy_recovery_staged_after_filtered(expected.mixed_deletions_position, None)
        .map_err(storage_error)?;
    let suffix = if staged.is_empty() {
        runtime
            .storage
            .legacy_recovery_suffix_after(expected.mixed_suffix_position)
            .map_err(storage_error)?
    } else {
        Vec::new()
    };
    if expected.mixed_suffix_count > expected.suffix_staged_count {
        return Err(failure("legacy suffix deletion checkpoint is inconsistent"));
    }
    let scanning_page = staged.is_empty() && suffix.is_empty();
    if scanning_page && expected.mixed_suffix_count != expected.suffix_staged_count {
        return Err(failure("legacy suffix deletion evidence is incomplete"));
    }
    let rows = if scanning_page {
        originals.iter().collect::<Vec<_>>()
    } else if staged.is_empty() {
        suffix.iter().map(|(_, row)| row).collect::<Vec<_>>()
    } else {
        staged.iter().map(|(_, row)| row).collect::<Vec<_>>()
    };
    let previous_snapshot = runtime.snapshot.clone();
    let previous_tombstones = runtime.tombstones.checkpoint();
    let result = (|| {
        runtime.tombstones.reconcile_ready_repositories(ready)?;
        let mut mutation = PendingRepositoryMutation {
            target_repository_id: local.into(),
            progress_at_unix_seconds: Some(now),
            ..PendingRepositoryMutation::default()
        };
        for row in rows {
            if row.identity != *identity || hex::encode(Sha256::digest(&row.wire)) != row.id {
                return Err(failure(
                    "legacy deletion evidence identity or digest mismatch",
                ));
            }
            let signed = SignedSegment::from_wire(&row.wire)?;
            signed.verify_identity(identity)?;
            let segment = signed.canonical();
            let first = segment.first_cursor();
            if segment.cluster_id() != cluster
                || first.source_node_id() != identity.node_id().as_str()
                || first.source_epoch() != expected.epoch
                || first.stream() != row.stream
            {
                return Err(failure("legacy deletion evidence scope mismatch"));
            }
            for (offset, record) in segment.records().iter().enumerate() {
                if !record.is_tombstone() || !is_known_schema(record) {
                    continue;
                }
                let sequence = first
                    .sequence()
                    .checked_add(
                        u64::try_from(offset)
                            .map_err(|_| RepositoryRuntimeError::StateLimitExceeded)?,
                    )
                    .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
                let cursor = ReplicaCursor::new(
                    first.source_node_id(),
                    first.source_epoch(),
                    first.stream(),
                    sequence,
                )?;
                let (schema, version) = record.schema();
                let source_record = ReplicaRecord::new(
                    &cursor,
                    record.subject_node_id(),
                    record.observer_node_id(),
                    schema,
                    version,
                    record.record_key().to_vec(),
                    record.payload_bytes().to_vec(),
                )?;
                let target_stream = source_stream_for_schema(schema)
                    .ok_or_else(|| failure("legacy tombstone schema has no target stream"))?;
                let target_cursor = ReplicaCursor::new(
                    first.source_node_id(),
                    first.source_epoch(),
                    target_stream,
                    sequence,
                )?;
                let target = ReplicaRecord::new(
                    &target_cursor,
                    record.subject_node_id(),
                    record.observer_node_id(),
                    schema,
                    version,
                    record.record_key().to_vec(),
                    record.payload_bytes().to_vec(),
                )?;
                runtime.delete_records_for_tombstone(&target.key(), &mut mutation)?;
                runtime.protect_tombstone_origin(&source_record.key(), &mut mutation)?;
                // Evidence is durable here; the local receipt/ACK is recorded only when the
                // unchanged signed segment is subsequently accepted by the receiver.
                runtime
                    .tombstones
                    .tombstone(source_record.key(), now, ready)?;
                let stored = StoredRecord::from_record(
                    segment.closed_at_unix_seconds(),
                    now,
                    &cursor,
                    record,
                );
                runtime.reset_partition_summary_cache();
                mutation.records.push(stored.sqlite_row()?);
            }
        }
        let mut next = expected.clone();
        if scanning_page {
            next.mixed_deletions_done = true;
        } else if staged.is_empty() {
            next.mixed_suffix_position = suffix.last().expect("nonempty suffix").0;
            next.mixed_suffix_count = next
                .mixed_suffix_count
                .checked_add(
                    u64::try_from(suffix.len())
                        .map_err(|_| RepositoryRuntimeError::StateLimitExceeded)?,
                )
                .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
        } else {
            next.mixed_deletions_position = staged.last().expect("nonempty stage").0;
        }
        if next.mixed_deletions_at.is_none() {
            next.mixed_deletions_at = Some(now);
        }
        next.mixed_ledger_hash = Some(ledger_hash(runtime)?);
        runtime.snapshot.tombstones = runtime.tombstones.checkpoint();
        let control =
            serde_json::to_vec(&runtime.snapshot_for_persistence()).map_err(storage_error)?;
        if control.len() > MAX_RUNTIME_STATE_BYTES {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        let outcome = runtime
            .storage
            .commit_repository_legacy_recovery(
                mutation.into_storage_mutation(),
                &control,
                Some(&encode(expected)?),
                &encode(&next)?,
                &[],
                &[],
                &[],
                false,
            )
            .map_err(storage_error)?;
        runtime.storage_degraded |= outcome.maintenance_degraded;
        Ok(false)
    })();
    if result.is_err() {
        runtime.snapshot = previous_snapshot;
        runtime.tombstones = TombstoneLedger::from_checkpoint(previous_tombstones)?;
    }
    result
}

impl RepositoryReplicaRuntime {
    pub(super) fn legacy_recovery_stage_local_tombstone(
        &mut self,
        cluster: &str,
        identity: &RepositoryNodeIdentity,
        state: LegacyRecoveryState,
    ) -> Result<LegacyRecoveryState, RepositoryRuntimeError> {
        if state.phase != "scan" || state.suffix_from_peer || !state.pending.is_empty() {
            return Ok(state);
        }
        let Some(target) = state.targets.get("tombstone") else {
            return Ok(state);
        };
        if target.next == target.head {
            return Ok(state);
        }
        let Some(row) = self
            .storage
            .legacy_recovery_local_tombstone(identity.node_id().as_str(), state.epoch, target.next)
            .map_err(storage_error)?
        else {
            return Ok(state);
        };
        if row.identity != *identity || hex::encode(Sha256::digest(&row.wire)) != row.id {
            return Err(failure(
                "legacy local tombstone identity or digest mismatch",
            ));
        }
        let signed = SignedSegment::from_wire(&row.wire)?;
        signed.verify_identity(identity)?;
        let canonical = signed.canonical();
        let first = canonical.first_cursor();
        if canonical.cluster_id() != cluster
            || first.source_node_id() != identity.node_id().as_str()
            || first.source_epoch() != state.epoch
            || first.stream() != "tombstone"
            || first.sequence() != target.next
            || canonical.previous_segment_hash() != target.hash
        {
            return Err(failure("legacy local tombstone chain mismatch"));
        }
        let end = canonical
            .last_cursor()
            .sequence()
            .checked_add(1)
            .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
        let hash = Some(signed.segment_hash()?);
        if end > target.head || (end == target.head && hash != target.predecessor) {
            return Err(failure("legacy local tombstone tail mismatch"));
        }
        let mut next = state.clone();
        next.mixed_deletions_required |= requires_prepass(canonical);
        if next.mixed_deletions_required {
            next.mixed_deletions_done = false;
        }
        let target = next.targets.get_mut("tombstone").expect("target exists");
        target.next = end;
        target.hash = hash;
        if next
            .targets
            .values()
            .all(|target| target.next == target.head && target.hash == target.predecessor)
        {
            next.phase = "apply".into();
        }
        self.legacy_recovery_save(Some(&state), &next, &[row], &[], false)?;
        Ok(next)
    }
}
