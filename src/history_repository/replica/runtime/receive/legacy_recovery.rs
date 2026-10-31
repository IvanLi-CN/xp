use super::*;
use crate::state::history_storage::SourceDeliveryJournalRow;
type RetainedAuthority<'a> = Option<(&'a [(String, [u8; 32])], u64)>;

#[path = "legacy_recovery_authority.rs"]
mod authority;
#[path = "legacy_recovery_deletions.rs"]
mod deletions;
#[path = "legacy_recovery_page.rs"]
mod page;
#[path = "legacy_recovery_suffix.rs"]
mod suffix;

#[cfg(test)]
#[path = "legacy_recovery_tests.rs"]
mod tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RecoveryTarget {
    pub(crate) head: u64,
    pub(crate) predecessor: Option<[u8; 32]>,
    pub(crate) next: u64,
    pub(crate) hash: Option<[u8; 32]>,
    // A signed suffix links the in-flight page to the durable source checkpoint captured
    // at recovery start. It is checked independently of the predecessor import.
    pub(crate) page_end: u64,
    pub(crate) page_hash: Option<[u8; 32]>,
    pub(crate) source_end: u64,
    pub(crate) source_hash: Option<[u8; 32]>,
    pub(crate) suffix_next: u64,
    pub(crate) suffix_hash: Option<[u8; 32]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LegacyRecoveryState {
    pub(crate) version: u8,
    pub(crate) page_key: String,
    pub(crate) epoch: u64,
    pub(crate) phase: String,
    pub(crate) targets: BTreeMap<String, RecoveryTarget>,
    pub(crate) peer_id: Option<String>,
    pub(crate) peer_url_hash: Option<String>,
    pub(crate) after_peer: Option<String>,
    pub(crate) cursor: Option<String>,
    pub(crate) pending: Vec<String>,
    pub(crate) next_cursor: Option<String>,
    pub(crate) retry_after: u64,
    pub(crate) apply_position: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) tombstone_apply_position: u64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub(crate) tombstones_applied: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) tombstones_applied_at: Option<u64>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub(crate) mixed_deletions_required: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) mixed_deletions_position: u64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub(crate) mixed_deletions_done: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) mixed_deletions_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) mixed_ledger_hash: Option<[u8; 32]>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) mixed_evidence_version: u64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub(crate) suffix_deletions_unavailable: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) suffix_capture_version: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) mixed_suffix_position: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) mixed_suffix_count: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) suffix_apply_position: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) suffix_staged_count: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) suffix_applied_count: u64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub(crate) page_receipted: bool,
    #[serde(default)]
    pub(crate) suffix_from_peer: bool,
    #[serde(default, skip_serializing_if = "suffix::ReceiverProof::is_empty")]
    pub(crate) receiver_proof: suffix::ReceiverProof,
}

#[derive(Deserialize)]
struct OriginalPage {
    segment_ids: Vec<String>,
    #[serde(default)]
    segments: Vec<serde::de::IgnoredAny>,
    #[serde(default)]
    legacy_source_segment_ids: Vec<String>,
    page_cursor: Option<String>,
    completed: bool,
}

fn failure(message: &str) -> RepositoryRuntimeError {
    RepositoryRuntimeError::Storage(message.into())
}
fn is_zero(value: &u64) -> bool {
    *value == 0
}
fn is_false(value: &bool) -> bool {
    !value
}
fn storage_error(error: impl std::fmt::Display) -> RepositoryRuntimeError {
    failure(&error.to_string())
}
fn encode(state: &LegacyRecoveryState) -> Result<Vec<u8>, RepositoryRuntimeError> {
    let bytes = serde_json::to_vec(state).map_err(storage_error)?;
    if bytes.len() > 128 * 1024 {
        return Err(RepositoryRuntimeError::StateLimitExceeded);
    }
    Ok(bytes)
}

impl RepositoryReplicaRuntime {
    fn legacy_original_context(
        &self,
        cluster: &str,
        identity: &RepositoryNodeIdentity,
    ) -> Result<
        (
            OriginalPage,
            Vec<SourceDeliveryJournalRow>,
            LegacyRecoveryState,
        ),
        RepositoryRuntimeError,
    > {
        if !self.uses_sqlite_history() {
            return Err(failure(
                "legacy original recovery requires external SQLite history",
            ));
        }
        let inflight = self
            .snapshot
            .local_history_backfill_inflight
            .as_ref()
            .ok_or_else(|| failure("legacy original page is missing"))?;
        let bytes = serde_json::to_vec(inflight).map_err(storage_error)?;
        if bytes.len() > MAX_RUNTIME_STATE_BYTES {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        let page: OriginalPage = serde_json::from_slice(&bytes).map_err(storage_error)?;
        if !page.segments.is_empty()
            || !page.legacy_source_segment_ids.is_empty()
            || page.segment_ids.is_empty()
        {
            return Err(failure("legacy original recovery refuses converted pages"));
        }
        if (!page.completed && page.page_cursor.is_none())
            || self.snapshot.local_history_backfill_completed
            || (page.page_cursor.is_some()
                && page.page_cursor == self.snapshot.local_history_backfill_cursor)
        {
            return Err(failure("legacy original page cursor was already committed"));
        }
        let rows = self
            .storage
            .source_delivery_journal_by_ids(&page.segment_ids, 1024 * 1024)
            .map_err(storage_error)?;
        let epoch = self.snapshot.local_source.epoch();
        if self
            .snapshot
            .local_source
            .node_id()
            .is_some_and(|node_id| node_id != identity.node_id().as_str())
        {
            return Err(failure("legacy source checkpoint identity mismatch"));
        }
        let mut targets = BTreeMap::<String, RecoveryTarget>::new();
        let mut tails = BTreeMap::<String, (u64, Option<[u8; 32]>)>::new();
        let mut records = 0;
        let mut mixed_deletions_required = false;
        for row in &rows {
            if row.identity != *identity || hex::encode(Sha256::digest(&row.wire)) != row.id {
                return Err(failure(
                    "legacy original journal identity or digest mismatch",
                ));
            }
            let signed = SignedSegment::from_wire(&row.wire)?;
            signed.verify_identity(identity)?;
            let canonical = signed.canonical();
            let first = canonical.first_cursor();
            mixed_deletions_required |= deletions::requires_prepass(canonical);
            if canonical.cluster_id() != cluster
                || epoch == 0
                || first.source_epoch() != epoch
                || first.source_node_id() != identity.node_id().as_str()
                || first.stream() != row.stream
                || first.stream().contains("-backfill-")
            {
                return Err(failure("legacy original journal scope mismatch"));
            }
            targets
                .entry(first.stream().into())
                .or_insert(RecoveryTarget {
                    head: first.sequence(),
                    predecessor: canonical.previous_segment_hash(),
                    next: 0,
                    hash: None,
                    page_end: 0,
                    page_hash: None,
                    source_end: 0,
                    source_hash: None,
                    suffix_next: 0,
                    suffix_hash: None,
                });
            if let Some((next, hash)) = tails.get(first.stream())
                && (*next != first.sequence() || *hash != canonical.previous_segment_hash())
            {
                return Err(failure("legacy original page is not a continuous chain"));
            }
            tails.insert(
                first.stream().into(),
                (
                    canonical
                        .last_cursor()
                        .sequence()
                        .checked_add(1)
                        .ok_or(RepositoryRuntimeError::StateLimitExceeded)?,
                    Some(signed.segment_hash()?),
                ),
            );
            records += canonical.records().len();
        }
        if targets.len() > 16 || records > 128 {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        for (stream, tail) in tails {
            let Some((source_next, source_hash)) =
                self.snapshot.local_source.stream_checkpoint(&stream)
            else {
                return Err(failure(
                    "legacy original page has no durable source stream checkpoint",
                ));
            };
            if source_next < tail.0 || (source_next == tail.0 && source_hash != tail.1) {
                return Err(failure(
                    "legacy original page conflicts with the durable source checkpoint",
                ));
            }
            let target = targets.get_mut(&stream).expect("tail has a target");
            target.page_end = tail.0;
            target.page_hash = tail.1;
            target.source_end = source_next;
            target.source_hash = source_hash;
            target.suffix_next = tail.0;
            target.suffix_hash = tail.1;
            if self.snapshot.gaps.iter().any(|gap| {
                gap.permanent
                    && gap.source_node_id == identity.node_id().as_str()
                    && gap.source_epoch == epoch
                    && gap.stream == stream
                    && gap.first_sequence < tail.0
            }) {
                return Err(failure(
                    "legacy original recovery overlaps a permanent source gap",
                ));
            }
        }
        if self.snapshot.local_source.has_tombstone_backpressure_gap()
            || page::has_tombstone_backpressure_gap(&self.snapshot.local_source)?
        {
            return Err(failure("legacy source tombstone has a backpressure gap"));
        }
        match self.snapshot.local_source.stream_checkpoint("tombstone") {
            Some((end, Some(hash))) if end > 0 => {
                if self.snapshot.gaps.iter().any(|gap| {
                    gap.permanent
                        && gap.source_node_id == identity.node_id().as_str()
                        && gap.source_epoch == epoch
                        && gap.stream == "tombstone"
                        && gap.first_sequence < end
                }) {
                    return Err(failure("legacy source tombstone overlaps a permanent gap"));
                }
                targets.insert(
                    "tombstone".into(),
                    RecoveryTarget {
                        head: end,
                        predecessor: Some(hash),
                        next: 0,
                        hash: None,
                        page_end: end,
                        page_hash: Some(hash),
                        source_end: end,
                        source_hash: Some(hash),
                        suffix_next: end,
                        suffix_hash: Some(hash),
                    },
                );
            }
            None | Some((0, None)) if self.snapshot.local_source.has_never_emitted_tombstones() => {
            }
            _ => return Err(failure("legacy source tombstone checkpoint is incomplete")),
        }
        if targets.len() > 16 {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        let mut digest = Sha256::new();
        digest.update(cluster.as_bytes());
        digest.update(serde_json::to_vec(identity).map_err(storage_error)?);
        digest.update(&bytes);
        let state = LegacyRecoveryState {
            version: 1,
            page_key: hex::encode(digest.finalize()),
            epoch,
            phase: "scan".into(),
            targets,
            peer_id: None,
            peer_url_hash: None,
            after_peer: None,
            cursor: None,
            pending: Vec::new(),
            next_cursor: None,
            retry_after: 0,
            apply_position: 0,
            tombstone_apply_position: 0,
            tombstones_applied: false,
            tombstones_applied_at: None,
            mixed_deletions_required,
            mixed_deletions_position: 0,
            mixed_deletions_done: false,
            mixed_deletions_at: None,
            mixed_ledger_hash: None,
            mixed_evidence_version: 1,
            suffix_deletions_unavailable: false,
            suffix_capture_version: 1,
            mixed_suffix_position: 0,
            mixed_suffix_count: 0,
            suffix_apply_position: 0,
            suffix_staged_count: 0,
            suffix_applied_count: 0,
            page_receipted: false,
            suffix_from_peer: false,
            receiver_proof: suffix::ReceiverProof::default(),
        };
        let mut state = state;
        if state
            .targets
            .values()
            .all(|target| target.head == 0 && target.predecessor.is_none())
        {
            state.phase = "apply".into();
        }
        Ok((page, rows, state))
    }

    pub(crate) fn legacy_original_recovery_state(
        &mut self,
        cluster: &str,
        identity: &RepositoryNodeIdentity,
        now: u64,
    ) -> Result<LegacyRecoveryState, RepositoryRuntimeError> {
        self.legacy_original_recovery_state_inner(cluster, identity, now, None)
    }

    fn legacy_original_recovery_state_inner(
        &mut self,
        cluster: &str,
        identity: &RepositoryNodeIdentity,
        now: u64,
        authority: RetainedAuthority<'_>,
    ) -> Result<LegacyRecoveryState, RepositoryRuntimeError> {
        self.legacy_recovery_freshness(cluster, identity, now, authority)?;
        let (_, _, initial) = self.legacy_original_context(cluster, identity)?;
        if let Some(bytes) = self
            .storage
            .legacy_recovery_checkpoint()
            .map_err(storage_error)?
        {
            let mut state: LegacyRecoveryState =
                serde_json::from_slice(&bytes).map_err(storage_error)?;
            if state.version == 1 && state.phase == "done" && state.page_key != initial.page_key {
                self.legacy_recovery_save(Some(&state), &initial, &[], &[], true)?;
                return Ok(initial);
            }
            if state.version != 1
                || state.page_key != initial.page_key
                || state.epoch != initial.epoch
                || state.pending.len() > 256
            {
                return Err(failure(
                    "legacy recovery checkpoint does not match the original page",
                ));
            }
            deletions::upgrade_evidence_checkpoint(self, &mut state, &initial)?;
            suffix::upgrade_capture(self, &mut state)?;
            for (stream, target) in &initial.targets {
                if stream == "tombstone" {
                    continue;
                }
                let Some(saved) = state.targets.get(stream) else {
                    return Err(failure("legacy recovery target disappeared"));
                };
                if saved.head != target.head
                    || saved.predecessor != target.predecessor
                    || saved.next > saved.head
                    || saved.page_end != target.page_end
                    || saved.page_hash != target.page_hash
                    || saved.source_end > target.source_end
                    || (saved.source_end == target.source_end
                        && saved.source_hash != target.source_hash)
                    || saved.suffix_next < saved.page_end
                    || saved.suffix_next > saved.source_end
                    || (saved.suffix_next == saved.source_end
                        && saved.suffix_hash != saved.source_hash)
                {
                    return Err(failure("legacy recovery target checkpoint mismatch"));
                }
            }
            deletions::refresh_source_tails(self, &mut state, &initial)?;
            suffix::refresh_receiver_tail(self, identity, &mut state, &initial)?;
            if state
                .targets
                .keys()
                .any(|stream| !initial.targets.contains_key(stream))
            {
                return Err(failure("legacy recovery target disappeared"));
            }
            if let Some(current) = initial.targets.get("tombstone") {
                let previous = state.clone();
                let receiver_cursor =
                    Cursor::new(identity.node_id().as_str(), state.epoch, "tombstone", 0)?;
                let receiver_next = self
                    .receiver
                    .as_ref()
                    .map(|receiver| receiver.continuous_watermark(&receiver_cursor))
                    .transpose()?
                    .flatten()
                    .and_then(|watermark| watermark.sequence().checked_add(1));
                let saved = state
                    .targets
                    .entry("tombstone".into())
                    .or_insert_with(|| current.clone());
                if saved.head > current.head
                    || (saved.head == current.head && saved.predecessor != current.predecessor)
                    || saved.next > saved.head
                    || saved.page_end != saved.head
                    || saved.page_hash != saved.predecessor
                    || saved.source_end != saved.head
                    || saved.source_hash != saved.predecessor
                    || saved.suffix_next != saved.head
                    || saved.suffix_hash != saved.predecessor
                {
                    return Err(failure("legacy tombstone checkpoint mismatch"));
                }
                // Finish the captured prefix before adopting a later source tail. Otherwise a
                // busy source could keep moving the goal while the original scan is in flight.
                let extend = saved.head < current.head
                    && (previous.tombstones_applied
                        || receiver_next.is_some_and(|next| next > saved.head));
                if extend {
                    saved.head = current.head;
                    saved.predecessor = current.predecessor;
                    saved.page_end = current.head;
                    saved.page_hash = current.predecessor;
                    saved.source_end = current.head;
                    saved.source_hash = current.predecessor;
                    saved.suffix_next = current.head;
                    saved.suffix_hash = current.predecessor;
                }
                if !previous.targets.contains_key("tombstone") || extend {
                    state.phase = "scan".into();
                    state.tombstones_applied = false;
                    state.peer_id = None;
                    state.peer_url_hash = None;
                    state.after_peer = None;
                    state.cursor = None;
                    state.pending.clear();
                    state.next_cursor = None;
                    state.retry_after = 0;
                    state.suffix_from_peer = false;
                    self.legacy_recovery_save(Some(&previous), &state, &[], &[], false)?;
                }
            }
            return self.legacy_recovery_stage_local_tombstone(cluster, identity, state);
        }
        self.legacy_recovery_save(None, &initial, &[], &[], false)?;
        self.legacy_recovery_stage_local_tombstone(cluster, identity, initial)
    }

    pub(crate) fn legacy_recovery_update_scan(
        &mut self,
        expected: &LegacyRecoveryState,
        next: &LegacyRecoveryState,
    ) -> Result<(), RepositoryRuntimeError> {
        if expected.page_key != next.page_key
            || expected.phase != next.phase
            || serde_json::to_vec(&expected.targets).map_err(storage_error)?
                != serde_json::to_vec(&next.targets).map_err(storage_error)?
            || expected.apply_position != next.apply_position
            || expected.tombstone_apply_position != next.tombstone_apply_position
            || expected.tombstones_applied != next.tombstones_applied
            || expected.tombstones_applied_at != next.tombstones_applied_at
            || expected.mixed_deletions_required != next.mixed_deletions_required
            || expected.mixed_deletions_position != next.mixed_deletions_position
            || expected.mixed_deletions_done != next.mixed_deletions_done
            || expected.mixed_deletions_at != next.mixed_deletions_at
            || expected.mixed_ledger_hash != next.mixed_ledger_hash
            || expected.mixed_evidence_version != next.mixed_evidence_version
            || expected.suffix_deletions_unavailable != next.suffix_deletions_unavailable
            || expected.suffix_capture_version != next.suffix_capture_version
            || expected.mixed_suffix_position != next.mixed_suffix_position
            || expected.mixed_suffix_count != next.mixed_suffix_count
            || expected.suffix_apply_position != next.suffix_apply_position
            || expected.suffix_staged_count != next.suffix_staged_count
            || expected.suffix_applied_count != next.suffix_applied_count
            || expected.page_receipted != next.page_receipted
            || expected.suffix_from_peer != next.suffix_from_peer
            || expected.receiver_proof != next.receiver_proof
            || next.pending.len() > 256
        {
            return Err(failure("invalid legacy network checkpoint transition"));
        }
        self.legacy_recovery_save(Some(expected), next, &[], &[], false)
    }

    fn legacy_recovery_save(
        &mut self,
        expected: Option<&LegacyRecoveryState>,
        next: &LegacyRecoveryState,
        staged: &[SourceDeliveryJournalRow],
        ack: &[String],
        finish: bool,
    ) -> Result<(), RepositoryRuntimeError> {
        self.legacy_recovery_save_with_suffix(expected, next, staged, &[], ack, finish)
    }

    pub(crate) fn legacy_recovery_stage_response(
        &mut self,
        cluster: &str,
        identity: &RepositoryNodeIdentity,
        expected: &LegacyRecoveryState,
        batch: &RepositoryRepairBatch,
    ) -> Result<LegacyRecoveryState, RepositoryRuntimeError> {
        let (_, _, context) = self.legacy_original_context(cluster, identity)?;
        if context.page_key != expected.page_key
            || expected.suffix_capture_version != 1
            || (expected.phase != "scan" && !expected.suffix_from_peer)
            || expected.pending.is_empty()
            || batch.segments.len() > 64
            || batch.unavailable_segment_ids.len() > 64
            || batch
                .segments
                .iter()
                .map(|row| row.wire.len())
                .sum::<usize>()
                > 256 * 1024
        {
            return Err(failure("invalid legacy repair response bounds or context"));
        }
        let mut next = expected.clone();
        let mut delivered = BTreeSet::new();
        let mut staged = Vec::new();
        let mut suffix_rows = Vec::new();
        for item in &batch.segments {
            let id = hex::encode(Sha256::digest(&item.wire));
            if !expected.pending.contains(&id) || !delivered.insert(id.clone()) {
                return Err(failure(
                    "legacy repair returned an unrequested or duplicate wire",
                ));
            }
            if item.identity != *identity {
                continue;
            }
            let signed = SignedSegment::from_wire(&item.wire)?;
            signed.verify_identity(identity)?;
            let canonical = signed.canonical();
            let first = canonical.first_cursor();
            if canonical.cluster_id() != cluster || first.source_epoch() != expected.epoch {
                continue;
            }
            let Some(target) = next.targets.get_mut(first.stream()) else {
                continue;
            };
            if expected.suffix_from_peer {
                // A peer can legitimately return newer segments in the same response after
                // this stream reaches the checkpoint captured at recovery start.
                if target.suffix_next == target.source_end {
                    continue;
                }
                if canonical.last_cursor().sequence() < target.suffix_next
                    || first.sequence() > target.suffix_next
                {
                    continue;
                }
                if first.sequence() != target.suffix_next
                    || canonical.previous_segment_hash() != target.suffix_hash
                {
                    return Err(failure("legacy peer suffix chain conflict"));
                }
                let end = canonical
                    .last_cursor()
                    .sequence()
                    .checked_add(1)
                    .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
                let hash = Some(signed.segment_hash()?);
                if end > target.source_end
                    || (end == target.source_end && hash != target.source_hash)
                {
                    return Err(failure("legacy peer suffix source checkpoint conflict"));
                }
                target.suffix_next = end;
                target.suffix_hash = hash;
                next.mixed_deletions_required |= deletions::requires_prepass(canonical);
                if canonical.records().iter().any(SyncRecord::is_tombstone) {
                    next.mixed_deletions_required = true;
                }
                if next.mixed_deletions_required {
                    next.mixed_deletions_done = false;
                }
                suffix_rows.push(SourceDeliveryJournalRow {
                    id,
                    stream: first.stream().into(),
                    closed_at_unix_seconds: canonical.closed_at_unix_seconds(),
                    identity: item.identity.clone(),
                    wire: item.wire.clone(),
                });
                continue;
            }
            if canonical.last_cursor().sequence() < target.next || first.sequence() > target.next {
                continue;
            }
            if first.sequence() != target.next {
                return Err(failure("overlapping legacy prefix range"));
            }
            if canonical.last_cursor().sequence() >= target.head {
                continue;
            }
            if canonical.previous_segment_hash() != target.hash {
                return Err(failure("legacy prefix predecessor hash mismatch"));
            }
            target.next = canonical
                .last_cursor()
                .sequence()
                .checked_add(1)
                .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
            target.hash = Some(signed.segment_hash()?);
            if target.next == target.head && target.hash != target.predecessor {
                return Err(failure(
                    "legacy prefix does not anchor the original pending page",
                ));
            }
            next.mixed_deletions_required |= deletions::requires_prepass(canonical);
            if next.mixed_deletions_required {
                next.mixed_deletions_done = false;
            }
            staged.push(SourceDeliveryJournalRow {
                id,
                stream: first.stream().into(),
                closed_at_unix_seconds: canonical.closed_at_unix_seconds(),
                identity: item.identity.clone(),
                wire: item.wire.clone(),
            });
        }
        for id in &batch.unavailable_segment_ids {
            if !expected.pending.contains(id) || !delivered.insert(id.clone()) {
                return Err(failure("invalid unavailable legacy repair ID"));
            }
        }
        if delivered.is_empty() {
            return Err(failure("legacy repair did not advance"));
        }
        next.pending.retain(|id| !delivered.contains(id));
        if expected.suffix_from_peer
            && next.targets.iter().any(|(stream, target)| {
                expected
                    .targets
                    .get(stream)
                    .is_some_and(|before| target.suffix_next > before.suffix_next)
            })
        {
            next.suffix_from_peer = false;
            next.peer_id = None;
            next.peer_url_hash = None;
            next.after_peer = None;
            next.cursor = None;
            next.pending.clear();
            next.next_cursor = None;
            next.retry_after = 0;
        } else if !expected.suffix_from_peer
            && next
                .targets
                .values()
                .all(|target| target.next == target.head && target.hash == target.predecessor)
        {
            next.phase = "apply".into();
            next.pending.clear();
        }
        next.suffix_staged_count = next
            .suffix_staged_count
            .checked_add(
                u64::try_from(suffix_rows.len())
                    .map_err(|_| RepositoryRuntimeError::StateLimitExceeded)?,
            )
            .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
        self.legacy_recovery_save_with_suffix(
            Some(expected),
            &next,
            &staged,
            &suffix_rows,
            &[],
            false,
        )?;
        Ok(next)
    }

    /// Apply at most 64 staged wires, or atomically accept the original page and ACK its journal.
    pub(crate) fn legacy_recovery_apply_step(
        &mut self,
        cluster: &str,
        identity: &RepositoryNodeIdentity,
        expected: &LegacyRecoveryState,
        now: u64,
        ready: &[String],
        local: &str,
    ) -> Result<bool, RepositoryRuntimeError> {
        self.legacy_recovery_apply_step_inner(cluster, identity, expected, now, ready, local, None)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn legacy_recovery_apply_step_with_retained_baseline(
        &mut self,
        cluster: &str,
        identity: &RepositoryNodeIdentity,
        expected: &LegacyRecoveryState,
        now: u64,
        ready: &[String],
        local: &str,
        peers: &[(String, [u8; 32])],
        generation: u64,
    ) -> Result<bool, RepositoryRuntimeError> {
        self.legacy_recovery_apply_step_inner(
            cluster,
            identity,
            expected,
            now,
            ready,
            local,
            Some((peers, generation)),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn legacy_recovery_apply_step_inner(
        &mut self,
        cluster: &str,
        identity: &RepositoryNodeIdentity,
        expected: &LegacyRecoveryState,
        now: u64,
        ready: &[String],
        local: &str,
        authority: RetainedAuthority<'_>,
    ) -> Result<bool, RepositoryRuntimeError> {
        {
            self.legacy_recovery_freshness(cluster, identity, now, authority)?;
            let (page, originals, context) = self.legacy_original_context(cluster, identity)?;
            if expected.page_key != context.page_key
                || expected.phase != "apply"
                || expected.suffix_capture_version != 1
                || !expected.targets.values().all(|target| {
                    target.next == target.head
                        && target.hash == target.predecessor
                        && target.suffix_next == target.source_end
                        && target.suffix_hash == target.source_hash
                })
            {
                return Err(failure("legacy prefix proof is incomplete"));
            }
            if !suffix::verify_receiver_step(self, cluster, identity, expected, &originals)? {
                return Ok(false);
            }
            if expected.mixed_deletions_required {
                deletions::ensure_source_tails_current(expected, &context)?;
                if expected.suffix_deletions_unavailable {
                    return Err(failure("legacy suffix evidence requires rescan"));
                }
                if !expected.mixed_deletions_done
                    || expected
                        .mixed_deletions_at
                        .is_some_and(|time| time > now || now - time >= TOMBSTONE_HORIZON_SECONDS)
                    || !deletions::ledger_matches(self, expected)?
                {
                    return deletions::apply_step(
                        self, cluster, identity, expected, &originals, now, ready, local,
                    );
                }
            }
            let current_tombstone = context.targets.get("tombstone");
            let saved_tombstone = expected.targets.get("tombstone");
            let applying_tombstones = saved_tombstone.is_some() && !expected.tombstones_applied;
            if let Some(target) = saved_tombstone {
                self.validate_existing_tombstone_receiver(identity, expected.epoch, target.head)?;
            }
            if !applying_tombstones
                && current_tombstone.map(|target| (target.head, target.predecessor))
                    != saved_tombstone.map(|target| (target.head, target.predecessor))
            {
                return Err(failure(
                    "legacy tombstone source advanced; refresh recovery checkpoint",
                ));
            }
            if saved_tombstone.is_some()
                && expected
                    .tombstones_applied_at
                    .is_some_and(|time| time > now || now - time >= TOMBSTONE_HORIZON_SECONDS)
            {
                let mut next = expected.clone();
                next.tombstones_applied = false;
                next.tombstone_apply_position = 0;
                next.tombstones_applied_at = None;
                self.legacy_recovery_save(Some(expected), &next, &[], &[], false)?;
                return Ok(false);
            }
            let batch =
                suffix::select_apply_batch(self, expected, &originals, applying_tombstones)?;
            if batch.kind == suffix::ApplyKind::TombstonesDone {
                let mut next = expected.clone();
                next.tombstones_applied = true;
                if next.tombstones_applied_at.is_none() {
                    next.tombstones_applied_at = Some(now);
                }
                self.legacy_recovery_save(Some(expected), &next, &[], &[], false)?;
                return Ok(false);
            }
            let final_page = batch.kind == suffix::ApplyKind::Final;
            let rows = batch.rows;
            let previous_snapshot = self.snapshot.clone();
            let previous_receiver = self
                .receiver
                .as_ref()
                .map(SegmentReceiver::checkpoint)
                .transpose()?;
            let previous_tombstones = self.tombstones.checkpoint();
            let result = (|| {
                self.bind_cluster(cluster)?;
                self.ensure_receiver()?;
                let mut mutation = PendingRepositoryMutation {
                    target_repository_id: local.into(),
                    progress_at_unix_seconds: Some(now),
                    ..PendingRepositoryMutation::default()
                };
                for row in &rows {
                    if row.identity != *identity || hex::encode(Sha256::digest(&row.wire)) != row.id
                    {
                        return Err(failure("legacy staged wire identity or digest mismatch"));
                    }
                    let signed = SignedSegment::from_wire(&row.wire)?;
                    let first = signed.canonical().first_cursor();
                    let last = signed.canonical().last_cursor();
                    let watermark = self
                        .receiver
                        .as_ref()
                        .expect("receiver initialized")
                        .continuous_watermark(first)?;
                    let receipt = suffix::receive_row(
                        self,
                        cluster,
                        expected,
                        row,
                        identity,
                        now,
                        ready,
                        local,
                        &mut mutation,
                    )?;
                    if applying_tombstones
                        && watermark
                            .as_ref()
                            .is_some_and(|current| current.sequence() >= last.sequence())
                    {
                        // A durable duplicate receipt does not rebuild an expired deletion ledger.
                        // Reapply the authenticated tombstone records in this same mutation.
                        suffix::append_deletion_evidence(
                            self,
                            signed.canonical(),
                            now,
                            ready,
                            local,
                            &mut mutation,
                        )?;
                    }
                    if final_page && expected.page_receipted {
                        suffix::append_deletion_evidence(
                            self,
                            signed.canonical(),
                            now,
                            ready,
                            local,
                            &mut mutation,
                        )?;
                    }
                    if expected.mixed_deletions_required {
                        deletions::ensure_same_segment_deletions(signed.canonical(), &mutation)?;
                    }
                    if receipt.gap.is_some()
                        || receipt.acknowledgement.source_node_id != last.source_node_id()
                        || receipt.acknowledgement.source_epoch != last.source_epoch()
                        || receipt.acknowledgement.stream != last.stream()
                        || receipt.acknowledgement.sequence < last.sequence()
                    {
                        return Err(failure(
                            "legacy original receipt does not acknowledge the exact stream",
                        ));
                    }
                }
                let mut next = expected.clone();
                let mut ack = Vec::new();
                if final_page {
                    // Strict receiver receipts precede staging the ACK.
                    for row in &rows {
                        self.remove_local_source_pending_segment(&row.wire)?;
                    }
                    if self.snapshot.local_source.node_id().is_none() {
                        self.set_local_source_epoch(context.epoch, identity.node_id().as_str());
                    }
                    self.snapshot.local_history_backfill_inflight = None;
                    self.snapshot.local_history_backfill_cursor = page.page_cursor;
                    self.snapshot.local_history_backfill_completed = page.completed;
                    ack = page.segment_ids;
                    next.phase = "done".into();
                } else {
                    match batch.kind {
                        suffix::ApplyKind::Tombstones => {
                            next.tombstone_apply_position =
                                batch.position.expect("staged position");
                            if next.tombstones_applied_at.is_none() {
                                next.tombstones_applied_at = Some(now);
                            }
                        }
                        suffix::ApplyKind::Prefix => {
                            next.apply_position = batch.position.expect("staged position")
                        }
                        suffix::ApplyKind::Page => next.page_receipted = true,
                        suffix::ApplyKind::Suffix => {
                            next.suffix_apply_position = batch.position.expect("suffix position");
                            next.suffix_applied_count = next
                                .suffix_applied_count
                                .checked_add(
                                    u64::try_from(rows.len())
                                        .map_err(|_| RepositoryRuntimeError::StateLimitExceeded)?,
                                )
                                .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
                        }
                        _ => unreachable!("completed cases handled above"),
                    }
                }
                self.snapshot.receiver =
                    Some(self.receiver.as_ref().expect("receiver").checkpoint()?);
                self.snapshot.tombstones = self.tombstones.checkpoint();
                let control =
                    serde_json::to_vec(&self.snapshot_for_persistence()).map_err(storage_error)?;
                if control.len() > MAX_RUNTIME_STATE_BYTES {
                    return Err(RepositoryRuntimeError::StateLimitExceeded);
                }
                let outcome = self
                    .storage
                    .commit_repository_legacy_recovery(
                        mutation.into_storage_mutation(),
                        &control,
                        Some(&encode(expected)?),
                        &encode(&next)?,
                        &[],
                        &[],
                        &ack,
                        final_page,
                    )
                    .map_err(storage_error)?;
                self.storage_degraded |= outcome.maintenance_degraded;
                self.mark_delivery_metrics_dirty();
                Ok(final_page)
            })();
            if result.is_err() {
                self.snapshot = previous_snapshot;
                self.receiver = previous_receiver
                    .map(|checkpoint| {
                        SegmentReceiver::from_checkpoint(cluster, known_schemas(), checkpoint)
                    })
                    .transpose()?;
                self.tombstones = TombstoneLedger::from_checkpoint(previous_tombstones)?;
            }
            result
        }
    }
}
