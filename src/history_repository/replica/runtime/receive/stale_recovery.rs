use super::*;
use crate::state::history_repository::{
    MAX_INITIAL_BACKFILL_PAGE_BYTES, MAX_INITIAL_BACKFILL_PAGE_RECORDS,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

#[path = "stale_recovery_source.rs"]
mod source;
#[cfg(test)]
#[path = "stale_recovery_tests.rs"]
mod tests;

const MAX_PEERS: usize = 4096;
const MAX_STATE_BYTES: usize = 8 * 1024 * 1024;
const MAX_BASELINE_KEYS: usize = 8192;
type StaleRecoveryProgress = (u64, bool, Vec<(String, Option<String>, bool)>);

#[derive(Deserialize)]
struct OriginalPage {
    segment_ids: Vec<String>,
    #[serde(default)]
    segments: Vec<serde::de::IgnoredAny>,
    #[serde(default)]
    legacy_source_segment_ids: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Peer {
    origin_hash: [u8; 32],
    cursor: Option<String>,
    done: bool,
    #[serde(default)]
    source_snapshot: Option<String>,
    #[serde(default)]
    source_next: u64,
    #[serde(default)]
    source_hash: Option<[u8; 32]>,
    #[serde(default)]
    source_markers: BTreeSet<[u8; 32]>,
}

#[derive(Clone, Serialize, Deserialize)]
struct State {
    version: u8,
    generation: u64,
    #[serde(default)]
    started_at: u64,
    cluster: String,
    identity_hash: [u8; 32],
    page_hash: [u8; 32],
    source_epoch: u64,
    peers: BTreeMap<String, Peer>,
    baseline: BTreeSet<[u8; 32]>,
    sealed: bool,
}

#[derive(Deserialize)]
struct LedgerView {
    entries: Vec<LedgerEntry>,
}
#[derive(Deserialize)]
struct LedgerEntry {
    key: ReplicaRecordKey,
    state: LedgerState,
}
#[derive(Deserialize)]
struct LedgerState {
    created_at: u64,
    expires_at: u64,
}

fn failure(message: &str) -> RepositoryRuntimeError {
    RepositoryRuntimeError::Storage(message.into())
}
fn storage_error(error: impl std::fmt::Display) -> RepositoryRuntimeError {
    failure(&error.to_string())
}
fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn encode(state: &State) -> Result<Vec<u8>, RepositoryRuntimeError> {
    let bytes = serde_json::to_vec(state).map_err(storage_error)?;
    if bytes.len() > MAX_STATE_BYTES {
        return Err(RepositoryRuntimeError::StateLimitExceeded);
    }
    Ok(bytes)
}
fn key_hash(key: &ReplicaRecordKey) -> Result<[u8; 32], RepositoryRuntimeError> {
    Ok(digest(&serde_json::to_vec(key).map_err(storage_error)?))
}
fn ledger_keys(
    runtime: &RepositoryReplicaRuntime,
    now: u64,
) -> Result<BTreeSet<[u8; 32]>, RepositoryRuntimeError> {
    let bytes = serde_json::to_vec(&runtime.tombstones.checkpoint()).map_err(storage_error)?;
    if bytes.len() > MAX_RUNTIME_STATE_BYTES {
        return Err(RepositoryRuntimeError::StateLimitExceeded);
    }
    let view: LedgerView = serde_json::from_slice(&bytes).map_err(storage_error)?;
    if view.entries.len() > MAX_BASELINE_KEYS {
        return Err(RepositoryRuntimeError::StateLimitExceeded);
    }
    view.entries
        .iter()
        .filter(|entry| entry.state.created_at <= now && entry.state.expires_at > now)
        .map(|entry| key_hash(&entry.key))
        .collect()
}

fn current_page(
    runtime: &RepositoryReplicaRuntime,
    cluster: &str,
    identity: &RepositoryNodeIdentity,
) -> Result<([u8; 32], [u8; 32], u64), RepositoryRuntimeError> {
    if !runtime.uses_sqlite_history() || cluster.is_empty() || cluster.len() > 256 {
        return Err(failure("stale recovery requires a bound SQLite repository"));
    }
    let inflight = runtime
        .snapshot
        .local_history_backfill_inflight
        .as_ref()
        .ok_or_else(|| failure("stale recovery requires an original pending page"))?;
    let bytes = serde_json::to_vec(inflight).map_err(storage_error)?;
    if bytes.len() > MAX_RUNTIME_STATE_BYTES {
        return Err(RepositoryRuntimeError::StateLimitExceeded);
    }
    let page: OriginalPage = serde_json::from_slice(&bytes).map_err(storage_error)?;
    if page.segment_ids.is_empty()
        || !page.segments.is_empty()
        || !page.legacy_source_segment_ids.is_empty()
    {
        return Err(failure("stale recovery requires original journal IDs"));
    }
    let epoch = runtime.snapshot.local_source.epoch();
    if epoch == 0
        || runtime
            .snapshot
            .cluster_id
            .as_deref()
            .is_some_and(|id| id != cluster)
        || runtime
            .snapshot
            .local_source
            .node_id()
            .is_some_and(|id| id != identity.node_id().as_str())
    {
        return Err(failure("stale recovery local source binding changed"));
    }
    let rows = runtime
        .storage
        .source_delivery_journal_by_ids(&page.segment_ids, 1024 * 1024)
        .map_err(storage_error)?;
    let mut tails = BTreeMap::<String, (u64, Option<[u8; 32]>)>::new();
    let mut records = 0_usize;
    for row in rows {
        if row.identity != *identity || hex::encode(digest(&row.wire)) != row.id {
            return Err(failure(
                "stale recovery journal identity or digest mismatch",
            ));
        }
        let signed = SignedSegment::from_wire(&row.wire)?;
        signed.verify_identity(identity)?;
        let canonical = signed.canonical();
        let first = canonical.first_cursor();
        if canonical.cluster_id() != cluster
            || first.source_epoch() != epoch
            || first.stream() != row.stream
            || first.stream().contains("-backfill-")
        {
            return Err(failure("stale recovery original scope mismatch"));
        }
        if let Some((next, previous)) = tails.get(first.stream())
            && (*next != first.sequence() || *previous != canonical.previous_segment_hash())
        {
            return Err(failure("stale recovery page chain is not contiguous"));
        }
        records = records
            .checked_add(canonical.records().len())
            .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
        let end = canonical
            .last_cursor()
            .sequence()
            .checked_add(1)
            .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
        tails.insert(first.stream().into(), (end, Some(signed.segment_hash()?)));
    }
    if records > 128 || tails.len() > 16 {
        return Err(RepositoryRuntimeError::StateLimitExceeded);
    }
    for (stream, (end, hash)) in tails {
        let (current, current_hash) = runtime
            .snapshot
            .local_source
            .stream_checkpoint(&stream)
            .ok_or_else(|| failure("stale recovery source stream checkpoint is missing"))?;
        if current < end || (current == end && current_hash != hash) {
            return Err(failure(
                "stale recovery original does not match the source checkpoint",
            ));
        }
    }
    Ok((
        digest(&bytes),
        digest(&serde_json::to_vec(identity).map_err(storage_error)?),
        epoch,
    ))
}

fn peer_map(
    peers: &[(String, [u8; 32])],
) -> Result<BTreeMap<String, Peer>, RepositoryRuntimeError> {
    if peers.is_empty() || peers.len() > MAX_PEERS {
        return Err(failure("stale recovery has no usable Ready baseline"));
    }
    let mut result = BTreeMap::new();
    for (id, hash) in peers {
        Cursor::new(id, 1, "stale-recovery", 0)?;
        if result
            .insert(
                id.clone(),
                Peer {
                    origin_hash: *hash,
                    cursor: None,
                    done: false,
                    source_snapshot: None,
                    source_next: 0,
                    source_hash: None,
                    source_markers: BTreeSet::new(),
                },
            )
            .is_some()
        {
            return Err(failure("stale recovery Ready membership has duplicate IDs"));
        }
    }
    Ok(result)
}

fn membership_matches(state: &State, peers: &BTreeMap<String, Peer>) -> bool {
    state.peers.len() == peers.len()
        && peers.iter().all(|(id, peer)| {
            state
                .peers
                .get(id)
                .is_some_and(|saved| saved.origin_hash == peer.origin_hash)
        })
}
fn bound(
    runtime: &RepositoryReplicaRuntime,
    state: &State,
    cluster: &str,
    identity: &RepositoryNodeIdentity,
    peers: &BTreeMap<String, Peer>,
) -> Result<bool, RepositoryRuntimeError> {
    let (page_hash, identity_hash, epoch) = current_page(runtime, cluster, identity)?;
    Ok(state.version == 1
        && state.cluster == cluster
        && state.page_hash == page_hash
        && state.identity_hash == identity_hash
        && state.source_epoch == epoch
        && membership_matches(state, peers))
}

fn cursor_phase(
    next: Option<&str>,
    previous: Option<&str>,
    done: bool,
) -> Result<(), RepositoryRuntimeError> {
    match next {
        Some(value) => {
            if value.len() > 1024 {
                return Err(RepositoryRuntimeError::StateLimitExceeded);
            }
            validate_tiered_backfill_cursor(value, previous).map_err(storage_error)?;
            let bytes = URL_SAFE_NO_PAD.decode(value).map_err(storage_error)?;
            let cursor: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(storage_error)?;
            let phase = cursor["phase"].as_str();
            if cursor["keyset_version"].as_u64() != Some(2)
                || phase != Some(if done { "records" } else { "tombstones" })
            {
                return Err(failure("stale recovery cursor phase is invalid"));
            }
        }
        None if !done => return Err(failure("unfinished stale deletion page has no cursor")),
        None => {}
    }
    Ok(())
}

impl RepositoryReplicaRuntime {
    pub(super) fn retained_stale_generation_for_page(
        &self,
        cluster: &str,
        identity: &RepositoryNodeIdentity,
    ) -> Result<bool, RepositoryRuntimeError> {
        if !self.uses_sqlite_history() {
            return Ok(false);
        }
        let Some(bytes) = self
            .storage
            .stale_recovery_checkpoint()
            .map_err(storage_error)?
        else {
            return Ok(false);
        };
        let state: State = serde_json::from_slice(&bytes).map_err(storage_error)?;
        let (page_hash, identity_hash, epoch) = current_page(self, cluster, identity)?;
        Ok(state.cluster == cluster
            && state.page_hash == page_hash
            && state.identity_hash == identity_hash
            && state.source_epoch == epoch)
    }

    pub(crate) fn legacy_page_needs_retained_stale_recovery(
        &self,
        cluster: &str,
        identity: &RepositoryNodeIdentity,
        _now: u64,
    ) -> Result<bool, RepositoryRuntimeError> {
        if self.snapshot.local_history_backfill_inflight.is_none()
            || self.local_history_backfill_inflight_is_persisted()
        {
            return Ok(false);
        }
        // A recent timestamp may have been advanced by an unrelated receive. A legacy page
        // has no creation time, so an existing receiver always needs the retained baseline.
        Ok(self.retained_stale_generation_for_page(cluster, identity)?
            || self.receiver.is_some()
            || self.snapshot.receiver.is_some()
            || self.snapshot.last_verified_unix_seconds.is_some())
    }

    /// `peers` and each page must be supplied by the authenticated Ready-peer worker.
    pub(crate) fn begin_retained_stale_recovery(
        &mut self,
        cluster: &str,
        identity: &RepositoryNodeIdentity,
        peers: &[(String, [u8; 32])],
        now: u64,
    ) -> Result<u64, RepositoryRuntimeError> {
        let expected_peers = peer_map(peers)?;
        let (page_hash, identity_hash, source_epoch) = current_page(self, cluster, identity)?;
        let previous = self
            .storage
            .stale_recovery_checkpoint()
            .map_err(storage_error)?;
        let old: Option<State> = previous
            .as_deref()
            .map(serde_json::from_slice)
            .transpose()
            .map_err(storage_error)?;
        if let Some(old) = &old {
            if old.version != 1 {
                return Err(failure("unsupported stale recovery checkpoint"));
            }
            if !old.sealed
                && (old.page_hash != page_hash
                    || old.identity_hash != identity_hash
                    || old.source_epoch != source_epoch
                    || old.cluster != cluster)
            {
                return Err(failure("pending stale recovery page changed"));
            }
            if bound(self, old, cluster, identity, &expected_peers)?
                && old.started_at <= now
                && now - old.started_at < TOMBSTONE_HORIZON_SECONDS
                && old.baseline.is_subset(&ledger_keys(self, now)?)
            {
                return Ok(old.generation);
            }
        }
        let generation = old
            .as_ref()
            .map_or(Some(1), |old| old.generation.checked_add(1))
            .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
        let next = State {
            version: 1,
            generation,
            started_at: now,
            cluster: cluster.into(),
            identity_hash,
            page_hash,
            source_epoch,
            peers: expected_peers,
            baseline: BTreeSet::new(),
            sealed: false,
        };
        self.commit_stale_recovery(
            previous.as_deref(),
            &next,
            PendingRepositoryMutation::default(),
        )?;
        Ok(generation)
    }

    pub(crate) fn retained_stale_recovery_progress(
        &self,
        cluster: &str,
        identity: &RepositoryNodeIdentity,
        peers: &[(String, [u8; 32])],
    ) -> Result<Option<StaleRecoveryProgress>, RepositoryRuntimeError> {
        let Some(bytes) = self
            .storage
            .stale_recovery_checkpoint()
            .map_err(storage_error)?
        else {
            return Ok(None);
        };
        let state: State = serde_json::from_slice(&bytes).map_err(storage_error)?;
        if !bound(self, &state, cluster, identity, &peer_map(peers)?)? {
            return Err(failure("stale recovery binding changed"));
        }
        Ok(Some((
            state.generation,
            state.sealed,
            state
                .peers
                .iter()
                .map(|(id, peer)| (id.clone(), peer.cursor.clone(), peer.done))
                .collect(),
        )))
    }

    pub(crate) fn restart_retained_stale_peer(
        &mut self,
        cluster: &str,
        identity: &RepositoryNodeIdentity,
        peers: &[(String, [u8; 32])],
        generation: u64,
        peer_id: &str,
        expected_cursor: Option<&str>,
    ) -> Result<(), RepositoryRuntimeError> {
        let bytes = self
            .storage
            .stale_recovery_checkpoint()
            .map_err(storage_error)?
            .ok_or_else(|| failure("stale recovery generation is missing"))?;
        let mut state: State = serde_json::from_slice(&bytes).map_err(storage_error)?;
        if state.generation != generation
            || state.sealed
            || !bound(self, &state, cluster, identity, &peer_map(peers)?)?
        {
            return Err(failure("stale recovery generation changed"));
        }
        let peer = state
            .peers
            .get_mut(peer_id)
            .ok_or_else(|| failure("stale recovery peer changed"))?;
        if peer.done || peer.cursor.as_deref() != expected_cursor {
            return Err(failure("stale recovery peer cursor changed"));
        }
        peer.cursor = None;
        peer.source_snapshot = None;
        self.commit_stale_recovery(Some(&bytes), &state, PendingRepositoryMutation::default())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn import_retained_stale_deletions(
        &mut self,
        cluster: &str,
        identity: &RepositoryNodeIdentity,
        peers: &[(String, [u8; 32])],
        generation: u64,
        peer_id: &str,
        prior: Option<&str>,
        next_cursor: Option<&str>,
        done: bool,
        records: Vec<RepositoryTieredBackfillRecord>,
        now: u64,
        local: &str,
    ) -> Result<(), RepositoryRuntimeError> {
        if local != identity.node_id().as_str() {
            return Err(failure("stale recovery local repository identity mismatch"));
        }
        if records.len() > MAX_INITIAL_BACKFILL_PAGE_RECORDS
            || records.iter().any(|row| !row.tombstone)
        {
            return Err(failure(
                "stale recovery page is not bounded tombstone-only data",
            ));
        }
        let raw_bytes = records
            .iter()
            .try_fold(0_usize, |total, row| {
                total
                    .checked_add(row.record_key.len())?
                    .checked_add(row.payload.len())
            })
            .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
        if raw_bytes > MAX_INITIAL_BACKFILL_PAGE_BYTES {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        let mut wire_bytes = 0_usize;
        for row in &records {
            wire_bytes = wire_bytes
                .checked_add(tiered_backfill_record_bytes(cluster, row)?)
                .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
            if wire_bytes > MAX_INITIAL_BACKFILL_PAGE_BYTES {
                return Err(RepositoryRuntimeError::StateLimitExceeded);
            }
        }
        let bytes = self
            .storage
            .stale_recovery_checkpoint()
            .map_err(storage_error)?
            .ok_or_else(|| failure("stale recovery generation is missing"))?;
        let mut state: State = serde_json::from_slice(&bytes).map_err(storage_error)?;
        if state.generation != generation
            || state.sealed
            || state.started_at > now
            || now - state.started_at >= TOMBSTONE_HORIZON_SECONDS
            || !bound(self, &state, cluster, identity, &peer_map(peers)?)?
        {
            return Err(failure("stale recovery generation changed"));
        }
        let peer = state
            .peers
            .get(peer_id)
            .ok_or_else(|| failure("stale recovery peer is not Ready"))?;
        if peer.done || peer.cursor.as_deref() != prior {
            return Err(failure("stale recovery peer cursor changed"));
        }
        cursor_phase(next_cursor, prior, done)?;
        self.refresh_capacity()?;
        let capacity = &self.snapshot.capacity;
        let required =
            crate::state::history_repository::control::HISTORY_RECOVERY_PAGE_BUDGET_BYTES
                .saturating_add(
                crate::state::history_repository::control::HISTORY_RECOVERY_METADATA_BUDGET_BYTES,
            );
        let filesystem_required =
            crate::state::history_repository::control::HISTORY_REPOSITORY_LOW_SPACE_GUARD_BYTES
                .saturating_add(required);
        if !capacity
            .history_write_availability()
            .allows_history_writes()
            || capacity.quota_bytes().saturating_sub(capacity.used_bytes()) < required
            || capacity.filesystem_available_bytes() < filesystem_required
        {
            return Err(RepositoryRuntimeError::WriteStopped(
                capacity.history_write_availability(),
            ));
        }
        let previous_snapshot = self.snapshot.clone();
        let previous_tombstones = self.tombstones.checkpoint();
        let result = (|| {
            let mut mutation = PendingRepositoryMutation {
                target_repository_id: local.into(),
                progress_at_unix_seconds: Some(now),
                ..PendingRepositoryMutation::default()
            };
            let mut ready = peers.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
            if !ready.iter().any(|id| id == local) {
                ready.push(local.to_owned());
            }
            for row in records {
                let cursor = ReplicaCursor::new(
                    &row.source_node_id,
                    row.source_epoch,
                    &row.stream,
                    row.sequence,
                )?;
                let record = SyncRecord::new(
                    &row.subject_node_id,
                    &row.observer_node_id,
                    &row.schema_id,
                    row.schema_version,
                    row.record_key,
                    row.payload,
                    true,
                );
                if !is_known_schema(&record) {
                    return Err(failure("stale recovery tombstone schema is unknown"));
                }
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
                    .ok_or_else(|| failure("stale recovery tombstone has no target stream"))?;
                let target_stream =
                    super::super::error::backfill_target_stream(&row.stream, target_stream)?;
                let target_cursor = ReplicaCursor::new(
                    &row.source_node_id,
                    row.source_epoch,
                    &target_stream,
                    row.sequence,
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
                self.delete_records_for_tombstone(&target.key(), &mut mutation)?;
                self.protect_tombstone_origin(&source_record.key(), &mut mutation)?;
                self.tombstones
                    .tombstone(source_record.key(), now, &ready)?;
                self.tombstones.acknowledge(source_record.key(), local)?;
                state.baseline.insert(key_hash(&source_record.key())?);
                if state.baseline.len() > MAX_BASELINE_KEYS {
                    return Err(RepositoryRuntimeError::StateLimitExceeded);
                }
                self.reset_partition_summary_cache();
                mutation.records.push(
                    StoredRecord::from_record(row.observed_at_unix_seconds, now, &cursor, &record)
                        .sqlite_row()?,
                );
                self.record_source_received_from_cursor(&cursor, now);
            }
            let peer = state.peers.get_mut(peer_id).expect("validated peer");
            peer.cursor = next_cursor.map(str::to_owned);
            peer.done = done;
            self.snapshot.tombstones = self.tombstones.checkpoint();
            self.commit_stale_recovery(Some(&bytes), &state, mutation)
        })();
        if result.is_err() {
            self.snapshot = previous_snapshot;
            self.tombstones = TombstoneLedger::from_checkpoint(previous_tombstones)?;
        }
        result
    }

    pub(crate) fn seal_retained_stale_recovery(
        &mut self,
        cluster: &str,
        identity: &RepositoryNodeIdentity,
        peers: &[(String, [u8; 32])],
        generation: u64,
        now: u64,
    ) -> Result<(), RepositoryRuntimeError> {
        let bytes = self
            .storage
            .stale_recovery_checkpoint()
            .map_err(storage_error)?
            .ok_or_else(|| failure("stale recovery generation is missing"))?;
        let mut state: State = serde_json::from_slice(&bytes).map_err(storage_error)?;
        if state.generation != generation
            || state.sealed
            || state.started_at > now
            || now - state.started_at >= TOMBSTONE_HORIZON_SECONDS
            || !bound(self, &state, cluster, identity, &peer_map(peers)?)?
            || state.peers.values().any(|peer| !peer.done)
            || !state.baseline.is_subset(&ledger_keys(self, now)?)
        {
            return Err(failure(
                "stale recovery retained deletion baseline is incomplete",
            ));
        }
        state.sealed = true;
        self.commit_stale_recovery(Some(&bytes), &state, PendingRepositoryMutation::default())
    }

    /// This authorizes only the bound pending page, not repository readiness or arbitrary replay.
    pub(crate) fn retained_stale_page_authorized(
        &self,
        cluster: &str,
        identity: &RepositoryNodeIdentity,
        peers: &[(String, [u8; 32])],
        generation: u64,
        now: u64,
    ) -> Result<bool, RepositoryRuntimeError> {
        let Some(bytes) = self
            .storage
            .stale_recovery_checkpoint()
            .map_err(storage_error)?
        else {
            return Ok(false);
        };
        let state: State = serde_json::from_slice(&bytes).map_err(storage_error)?;
        Ok(state.generation == generation
            && state.sealed
            && state.started_at <= now
            && now - state.started_at < TOMBSTONE_HORIZON_SECONDS
            && bound(self, &state, cluster, identity, &peer_map(peers)?)?
            && state.baseline.is_subset(&ledger_keys(self, now)?))
    }

    fn commit_stale_recovery(
        &mut self,
        expected: Option<&[u8]>,
        next: &State,
        mutation: PendingRepositoryMutation,
    ) -> Result<(), RepositoryRuntimeError> {
        let control =
            serde_json::to_vec(&self.snapshot_for_persistence()).map_err(storage_error)?;
        if control.len() > MAX_RUNTIME_STATE_BYTES {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        let result = self
            .storage
            .commit_repository_stale_recovery(
                mutation.into_storage_mutation(),
                &control,
                expected,
                &encode(next)?,
            )
            .map_err(storage_error)?;
        self.storage_degraded |= result.maintenance_degraded;
        Ok(())
    }
}
