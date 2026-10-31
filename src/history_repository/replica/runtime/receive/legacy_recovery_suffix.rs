use super::*;

#[path = "legacy_receiver_proof.rs"]
mod receiver_proof;
pub(super) use receiver_proof::{
    ReceiverProof, append_deletion_evidence, receive_row, verify_receiver_step,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ApplyKind {
    TombstonesDone,
    Tombstones,
    Prefix,
    Page,
    Suffix,
    Final,
}

pub(super) struct ApplyBatch {
    pub(super) kind: ApplyKind,
    pub(super) rows: Vec<SourceDeliveryJournalRow>,
    pub(super) position: Option<u64>,
}

pub(super) fn select_apply_batch(
    runtime: &RepositoryReplicaRuntime,
    state: &LegacyRecoveryState,
    originals: &[SourceDeliveryJournalRow],
    applying_tombstones: bool,
) -> Result<ApplyBatch, RepositoryRuntimeError> {
    if state.suffix_applied_count > state.suffix_staged_count
        || (!state.page_receipted
            && (state.suffix_apply_position != 0 || state.suffix_applied_count != 0))
    {
        return Err(failure("legacy suffix receipt checkpoint is inconsistent"));
    }
    let staged = runtime
        .storage
        .legacy_recovery_staged_after_filtered(
            if applying_tombstones {
                state.tombstone_apply_position
            } else {
                state.apply_position
            },
            Some(applying_tombstones),
        )
        .map_err(storage_error)?;
    if !staged.is_empty() {
        return Ok(ApplyBatch {
            kind: if applying_tombstones {
                ApplyKind::Tombstones
            } else {
                ApplyKind::Prefix
            },
            position: staged.last().map(|(position, _)| *position),
            rows: staged.into_iter().map(|(_, row)| row).collect(),
        });
    }
    if applying_tombstones {
        return Ok(ApplyBatch {
            kind: ApplyKind::TombstonesDone,
            rows: Vec::new(),
            position: None,
        });
    }
    page::validate_tombstone_wires(runtime, originals)?;
    let suffix = runtime
        .storage
        .legacy_recovery_suffix_after(state.suffix_apply_position)
        .map_err(storage_error)?;
    if suffix.is_empty() && state.suffix_applied_count != state.suffix_staged_count {
        return Err(failure("legacy verified suffix staging is incomplete"));
    }
    if !suffix.is_empty() && state.page_receipted {
        return Ok(ApplyBatch {
            kind: ApplyKind::Suffix,
            position: suffix.last().map(|(position, _)| *position),
            rows: suffix.into_iter().map(|(_, row)| row).collect(),
        });
    }
    Ok(ApplyBatch {
        kind: if suffix.is_empty() {
            ApplyKind::Final
        } else {
            ApplyKind::Page
        },
        rows: originals.to_vec(),
        position: None,
    })
}

pub(super) fn upgrade_capture(
    runtime: &mut RepositoryReplicaRuntime,
    state: &mut LegacyRecoveryState,
) -> Result<(), RepositoryRuntimeError> {
    if state.suffix_capture_version > 1 {
        return Err(failure("unsupported legacy suffix checkpoint"));
    }
    if state.suffix_capture_version == 1 {
        return Ok(());
    }
    let previous = state.clone();
    for (stream, target) in &mut state.targets {
        if stream != "tombstone" {
            target.suffix_next = target.page_end;
            target.suffix_hash = target.page_hash;
        }
    }
    state.suffix_capture_version = 1;
    state.suffix_deletions_unavailable = false;
    state.suffix_from_peer = false;
    state.peer_id = None;
    state.peer_url_hash = None;
    state.after_peer = None;
    state.cursor = None;
    state.pending.clear();
    state.next_cursor = None;
    state.retry_after = 0;
    state.mixed_deletions_done = false;
    state.mixed_deletions_position = 0;
    state.mixed_suffix_position = 0;
    state.mixed_suffix_count = 0;
    state.suffix_staged_count = 0;
    state.suffix_applied_count = 0;
    state.mixed_deletions_at = None;
    state.mixed_ledger_hash = None;
    runtime.legacy_recovery_save(Some(&previous), state, &[], &[], false)
}

impl RepositoryReplicaRuntime {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn legacy_recovery_save_with_suffix(
        &mut self,
        expected: Option<&LegacyRecoveryState>,
        next: &LegacyRecoveryState,
        staged: &[SourceDeliveryJournalRow],
        suffix: &[SourceDeliveryJournalRow],
        ack: &[String],
        finish: bool,
    ) -> Result<(), RepositoryRuntimeError> {
        let expected = expected.map(encode).transpose()?;
        let control = self
            .storage
            .read(REPOSITORY_REPLICA_KEY)
            .map_err(storage_error)?
            .ok_or_else(|| failure("legacy recovery control snapshot is missing"))?;
        if control.len() > MAX_RUNTIME_STATE_BYTES {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        self.storage
            .commit_repository_legacy_recovery(
                RepositoryReplicaMutation::default(),
                &control,
                expected.as_deref(),
                &encode(next)?,
                staged,
                suffix,
                ack,
                finish,
            )
            .map_err(storage_error)?;
        Ok(())
    }
}

impl RepositoryReplicaRuntime {
    pub(crate) fn legacy_recovery_verify_suffix_step(
        &mut self,
        cluster: &str,
        identity: &RepositoryNodeIdentity,
        expected: &LegacyRecoveryState,
    ) -> Result<bool, RepositoryRuntimeError> {
        if expected.suffix_capture_version != 1 {
            return Err(failure("legacy suffix checkpoint requires rescan"));
        }
        let Some((stream, target)) = expected
            .targets
            .iter()
            .find(|(_, target)| target.suffix_next < target.source_end)
        else {
            return Ok(true);
        };
        let previous_hash = target
            .suffix_hash
            .ok_or_else(|| failure("legacy suffix lacks a predecessor hash"))?;
        let page = self.storage.source_delivery_journal_successors(
            cluster,
            identity,
            expected.epoch,
            stream,
            target.suffix_next,
            target.source_end,
            previous_hash,
            64,
            256 * 1024,
        );
        let (suffix_rows, next_sequence, hash) = match page {
            Ok(page) => page,
            Err(error)
                if error.to_string() == "journal successor missing at expected sequence"
                    || error.to_string() == "journal successor missing before source tail" =>
            {
                // A real prior ACK may have removed the local wire. Search registered peers;
                // absence alone is not evidence of corruption or permission to skip a link.
                let mut next = expected.clone();
                next.suffix_from_peer = true;
                next.peer_id = None;
                next.peer_url_hash = None;
                next.after_peer = None;
                next.cursor = None;
                next.pending.clear();
                next.next_cursor = None;
                next.retry_after = 0;
                self.legacy_recovery_save(Some(expected), &next, &[], &[], false)?;
                return Ok(false);
            }
            Err(error) => return Err(storage_error(error)),
        };
        if next_sequence <= target.suffix_next
            || (next_sequence == target.source_end && Some(hash) != target.source_hash)
        {
            return Err(failure(
                "legacy suffix conflicts with durable source checkpoint",
            ));
        }
        let mut next = expected.clone();
        for row in &suffix_rows {
            let signed = SignedSegment::from_wire(&row.wire)?;
            next.mixed_deletions_required |= deletions::requires_prepass(signed.canonical());
            if signed
                .canonical()
                .records()
                .iter()
                .any(SyncRecord::is_tombstone)
            {
                next.mixed_deletions_required = true;
            }
        }
        if next.mixed_deletions_required && !suffix_rows.is_empty() {
            next.mixed_deletions_done = false;
        }
        next.suffix_staged_count = next
            .suffix_staged_count
            .checked_add(
                u64::try_from(suffix_rows.len())
                    .map_err(|_| RepositoryRuntimeError::StateLimitExceeded)?,
            )
            .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
        let target = next.targets.get_mut(stream).expect("suffix target exists");
        target.suffix_next = next_sequence;
        target.suffix_hash = Some(hash);
        self.legacy_recovery_save_with_suffix(
            Some(expected),
            &next,
            &[],
            &suffix_rows,
            &[],
            false,
        )?;
        Ok(next.targets.values().all(|target| {
            target.suffix_next == target.source_end && target.suffix_hash == target.source_hash
        }))
    }
}

// A live receiver may advance while a restartable proof is being scanned. Extend only a
// fully checked captured suffix, then authenticate the additional bounded pages normally.
pub(super) fn refresh_receiver_tail(
    runtime: &mut RepositoryReplicaRuntime,
    identity: &RepositoryNodeIdentity,
    state: &mut LegacyRecoveryState,
    current: &LegacyRecoveryState,
) -> Result<(), RepositoryRuntimeError> {
    let previous = state.clone();
    for (stream, saved) in &mut state.targets {
        if stream == "tombstone" || saved.suffix_next != saved.source_end {
            continue;
        }
        let cursor = Cursor::new(identity.node_id().as_str(), state.epoch, stream, 0)?;
        let tip = runtime
            .receiver
            .as_ref()
            .map(|receiver| receiver.continuous_watermark(&cursor))
            .transpose()?
            .flatten();
        let target = &current.targets[stream];
        if target.source_end > saved.source_end
            && tip.is_some_and(|tip| tip.sequence() >= saved.source_end)
        {
            saved.source_end = target.source_end;
            saved.source_hash = target.source_hash;
        }
    }
    if serde_json::to_vec(&state.targets).map_err(storage_error)?
        != serde_json::to_vec(&previous.targets).map_err(storage_error)?
    {
        state.receiver_proof = ReceiverProof::default();
        runtime.legacy_recovery_save(Some(&previous), state, &[], &[], false)?;
    }
    Ok(())
}
