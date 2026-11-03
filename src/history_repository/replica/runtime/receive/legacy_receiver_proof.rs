use super::*;

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ReceiverProof {
    scope: String,
    tips: BTreeMap<String, Tip>,
    phase: u8,
    position: u64,
    suffix_count: u64,
}

impl ReceiverProof {
    pub(in super::super) fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Tip {
    sequence: u64,
    hash: [u8; 32],
    next: u64,
    previous: Option<[u8; 32]>,
    matched: bool,
}

// Each call authenticates one existing bounded SQLite page. Only the checkpoint, never the
// full chain, survives in memory or across restarts. No materialization or ACK precedes proof.
pub(in super::super) fn verify_receiver_step(
    runtime: &mut RepositoryReplicaRuntime,
    cluster: &str,
    identity: &RepositoryNodeIdentity,
    state: &LegacyRecoveryState,
    originals: &[SourceDeliveryJournalRow],
) -> Result<bool, RepositoryRuntimeError> {
    if let Some(target) = state.targets.get("tombstone") {
        runtime.validate_existing_tombstone_receiver(identity, state.epoch, target.head)?;
    }
    let batch = select_apply_batch(
        runtime,
        state,
        originals,
        state.targets.contains_key("tombstone") && !state.tombstones_applied,
    )?;
    let Some(receiver) = runtime.receiver.as_ref() else {
        return Ok(true);
    };
    let mut needed = false;
    for row in &batch.rows {
        let signed = SignedSegment::from_wire(&row.wire)?;
        if receiver
            .continuous_watermark(signed.canonical().first_cursor())?
            .is_some_and(|tip| tip.sequence() >= signed.canonical().last_cursor().sequence())
            && !runtime.stored_segment_exists(&row.id)?
        {
            needed = true;
        }
    }
    if !needed {
        return Ok(true);
    }
    let mut tips = BTreeMap::new();
    for (stream, target) in &state.targets {
        let cursor = Cursor::new(identity.node_id().as_str(), state.epoch, stream, 0)?;
        if let Some((tip, hash)) = receiver.continuous_watermark_with_hash(&cursor)? {
            if tip.sequence() >= target.source_end {
                return Err(failure(concat!(
                    "legacy recovery cannot ACK an unknown wire ",
                    "below the receiver watermark: tip exceeds verified source tail"
                )));
            }
            if runtime.snapshot.gaps.iter().any(|gap| {
                gap.permanent
                    && gap.source_node_id == cursor.source_node_id()
                    && gap.source_epoch == state.epoch
                    && gap.stream == *stream
                    && gap.first_sequence < target.source_end
            }) {
                return Err(failure("legacy receiver proof overlaps a permanent gap"));
            }
            tips.insert(
                stream.clone(),
                Tip {
                    sequence: tip.sequence(),
                    hash,
                    next: 0,
                    previous: None,
                    matched: false,
                },
            );
        }
    }
    let scope = hex::encode(Sha256::digest(
        serde_json::to_vec(&(&state.page_key, state.epoch, &state.targets))
            .map_err(storage_error)?,
    ));
    let proof = &state.receiver_proof;
    if proof.scope != scope
        || proof.tips.len() != tips.len()
        || tips.iter().any(|(stream, tip)| {
            proof
                .tips
                .get(stream)
                .is_none_or(|saved| saved.sequence != tip.sequence || saved.hash != tip.hash)
        })
    {
        let mut next = state.clone();
        next.receiver_proof = ReceiverProof {
            scope,
            tips,
            ..ReceiverProof::default()
        };
        runtime.legacy_recovery_save(Some(state), &next, &[], &[], false)?;
        return Ok(false);
    }
    if proof.phase == 3 {
        return Ok(true);
    }
    page::validate_tombstone_wires(runtime, originals)?;
    let mut next = state.clone();
    let proof = &mut next.receiver_proof;
    let rows = match proof.phase {
        0 => runtime
            .storage
            .legacy_recovery_staged_after_filtered(proof.position, None)
            .map_err(storage_error)?,
        1 => originals.iter().cloned().map(|row| (0, row)).collect(),
        2 => runtime
            .storage
            .legacy_recovery_suffix_after(proof.position)
            .map_err(storage_error)?,
        _ => return Err(failure("invalid legacy receiver proof phase")),
    };
    for (position, row) in &rows {
        if row.identity != *identity || hex::encode(Sha256::digest(&row.wire)) != row.id {
            return Err(failure("legacy receiver proof identity or digest mismatch"));
        }
        let signed = SignedSegment::from_wire(&row.wire)?;
        signed.verify_identity(identity)?;
        let canonical = signed.canonical();
        let first = canonical.first_cursor();
        let last = canonical.last_cursor();
        if canonical.cluster_id() != cluster
            || first.source_node_id() != identity.node_id().as_str()
            || first.source_epoch() != state.epoch
            || row.stream != first.stream()
        {
            return Err(failure("legacy receiver proof scope mismatch"));
        }
        if let Some(tip) = proof.tips.get_mut(first.stream()) {
            // Original tombstone wires were already included and checked in the staged prefix.
            if !(proof.phase == 1 && first.stream() == "tombstone" && last.sequence() < tip.next) {
                if first.sequence() != tip.next || canonical.previous_segment_hash() != tip.previous
                {
                    return Err(failure(
                        "legacy receiver proof has a missing or conflicting link",
                    ));
                }
                let hash = signed.segment_hash()?;
                if first.sequence() <= tip.sequence && tip.sequence <= last.sequence() {
                    if last.sequence() != tip.sequence || hash != tip.hash {
                        return Err(failure("legacy receiver tip boundary or hash mismatch"));
                    }
                    tip.matched = true;
                }
                tip.next = last
                    .sequence()
                    .checked_add(1)
                    .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
                tip.previous = Some(hash);
            }
        }
        proof.position = *position;
        if proof.phase == 2 {
            proof.suffix_count += 1;
        }
    }
    if rows.is_empty() || proof.phase == 1 {
        proof.phase += 1;
        proof.position = 0;
        if proof.phase == 3
            && (proof.suffix_count != state.suffix_staged_count
                || proof.tips.iter().any(|(stream, tip)| {
                    let target = &state.targets[stream];
                    !tip.matched
                        || tip.next != target.source_end
                        || tip.previous != target.source_hash
                }))
        {
            return Err(failure(
                "legacy receiver proof does not reach both receiver and source tips",
            ));
        }
    }
    runtime.legacy_recovery_save(Some(state), &next, &[], &[], false)?;
    Ok(false)
}

#[allow(clippy::too_many_arguments)]
fn receive_verified(
    runtime: &mut RepositoryReplicaRuntime,
    state: &LegacyRecoveryState,
    row: &SourceDeliveryJournalRow,
    identity: &RepositoryNodeIdentity,
    now: u64,
    ready: &[String],
    local: &str,
    mutation: &mut PendingRepositoryMutation,
) -> Result<RepositorySyncReceipt, RepositoryRuntimeError> {
    let signed = SignedSegment::from_wire(&row.wire)?;
    let first = signed.canonical().first_cursor();
    let proof = &state.receiver_proof;
    let tip = proof
        .tips
        .get(first.stream())
        .filter(|tip| proof.phase == 3 && tip.matched)
        .ok_or_else(|| failure("legacy receiver proof is incomplete"))?;
    let cursor = Cursor::new(
        first.source_node_id(),
        first.source_epoch(),
        first.stream(),
        tip.sequence,
    )?;
    let acceptance = runtime
        .receiver
        .as_ref()
        .expect("receiver initialized")
        .accept_verified_history_duplicate(&signed, identity, &cursor, tip.hash)?;
    runtime.refresh_capacity()?;
    let availability = runtime.snapshot.capacity.history_write_availability();
    if !availability.allows_history_writes() {
        return Err(RepositoryRuntimeError::WriteStopped(availability));
    }
    runtime.tombstones.reconcile_ready_repositories(ready)?;
    runtime.expire_tombstones(now, mutation)?;
    // The continuous receipt proves ordinary records were already accepted. Their absence
    // may be a deliberate deletion or retention; never rebuild them or their expired wire cache.
    let acknowledgements =
        append_deletion_evidence(runtime, signed.canonical(), now, ready, local, mutation)?;
    Ok(sync_receipt(acceptance, availability, acknowledgements))
}

#[allow(clippy::too_many_arguments)]
pub(in super::super) fn receive_row(
    runtime: &mut RepositoryReplicaRuntime,
    cluster: &str,
    state: &LegacyRecoveryState,
    row: &SourceDeliveryJournalRow,
    identity: &RepositoryNodeIdentity,
    now: u64,
    ready: &[String],
    local: &str,
    mutation: &mut PendingRepositoryMutation,
) -> Result<RepositorySyncReceipt, RepositoryRuntimeError> {
    let signed = SignedSegment::from_wire(&row.wire)?;
    let canonical = signed.canonical();
    if runtime
        .receiver
        .as_ref()
        .expect("receiver initialized")
        .continuous_watermark(canonical.first_cursor())?
        .is_some_and(|tip| tip.sequence() >= canonical.last_cursor().sequence())
        && !runtime.stored_segment_exists(&row.id)?
    {
        return receive_verified(runtime, state, row, identity, now, ready, local, mutation);
    }
    runtime.receive_wire_from_repository_inner_without_rebuild(
        RepositoryWireRequest {
            cluster_id: cluster,
            identity,
            wire: &row.wire,
            now_unix_seconds: now,
            ready_repositories: ready,
            local_repository_id: local,
            allow_retained_anchor: false,
            rollback_snapshot: None,
            discard_gap_evidence_on_error: true,
            allow_retained_sequence_gap: false,
            retained_anchor_checkpoint: None,
        },
        Some(mutation),
        false,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
pub(in super::super) fn append_deletion_evidence(
    runtime: &mut RepositoryReplicaRuntime,
    segment: &CanonicalSegment,
    now: u64,
    ready: &[String],
    local: &str,
    mutation: &mut PendingRepositoryMutation,
) -> Result<Vec<RepositoryTombstoneAcknowledgement>, RepositoryRuntimeError> {
    let mut acknowledgements = Vec::new();
    for (offset, record) in segment.records().iter().enumerate() {
        if !record.is_tombstone() {
            continue;
        }
        let first = segment.first_cursor();
        let sequence = first
            .sequence()
            .checked_add(offset as u64)
            .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
        // This is only a materialization view. The authenticated wire is never changed or signed.
        // Retain each tombstone's original cursor, even in a mixed legacy segment.
        let view = CanonicalSegment::new(
            segment.cluster_id(),
            Cursor::new(
                first.source_node_id(),
                first.source_epoch(),
                first.stream(),
                sequence,
            )?,
            vec![record.clone()],
            segment.previous_segment_hash(),
            segment.opened_at_unix_seconds(),
            segment.closed_at_unix_seconds(),
        )?;
        acknowledgements
            .extend(runtime.append_known_records(&view, now, ready, local, mutation, true)?);
    }
    Ok(acknowledgements)
}
