use super::super::types::RepositoryWireRequest;
use super::*;

fn valid_source_marker(record: &SyncRecord, source: &str) -> bool {
    if !record.is_tombstone()
        || record.schema().1 != 1
        || record.subject_node_id() != record.observer_node_id()
    {
        return false;
    }
    let key = record.record_key();
    if key == format!("node-history:node:{}:", record.subject_node_id()).as_bytes() {
        return matches!(
            record.schema().0,
            "runtime.v1" | "path_health.v1" | "traffic.v1" | "connections.v1" | "ip_usage.v1"
        );
    }
    let Some(user) = key
        .strip_prefix(b"node-history:user:")
        .and_then(|key| key.strip_suffix(b":"))
    else {
        return false;
    };
    !user.is_empty() && record.schema().0 == "traffic.v1" && record.subject_node_id() == source
}

impl RepositoryReplicaRuntime {
    /// Store a source deletion page as local signed evidence and checkpoint it atomically.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn import_retained_stale_source_deletions(
        &mut self,
        cluster: &str,
        identity: &RepositoryNodeIdentity,
        signing_key: &ed25519_dalek::SigningKey,
        peers: &[(String, [u8; 32])],
        generation: u64,
        peer_id: &str,
        prior: Option<&str>,
        next: Option<&str>,
        snapshot: &str,
        records: Vec<(u64, SyncRecord)>,
        now: u64,
    ) -> Result<(), RepositoryRuntimeError> {
        if records.len() > MAX_INITIAL_BACKFILL_PAGE_RECORDS
            || records
                .iter()
                .any(|(_, record)| !valid_source_marker(record, peer_id))
            || next.is_some_and(|cursor| cursor.len() > 1024)
            || snapshot.len() > 128
        {
            return Err(failure("invalid source deletion page"));
        }
        let bytes = records
            .iter()
            .try_fold(0_usize, |total, (_, record)| {
                total
                    .checked_add(record.record_key().len())?
                    .checked_add(record.payload_bytes().len())?
                    .checked_add(record.subject_node_id().len())?
                    .checked_add(record.observer_node_id().len())
            })
            .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
        if bytes > MAX_INITIAL_BACKFILL_PAGE_BYTES {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
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
            .ok_or_else(|| failure("source binding changed"))?;
        if peer.done
            || peer.cursor.as_deref() != prior
            || peer
                .source_snapshot
                .as_deref()
                .is_some_and(|old| old != snapshot)
        {
            return Err(failure("source deletion snapshot or cursor changed"));
        }
        // A source may change between pages. A restart replays earlier prefixes, which
        // must reuse their existing durable evidence instead of consuming new ledger slots.
        let mut seen = peer.source_markers.clone();
        let mut unseen = Vec::with_capacity(records.len());
        for (observed, record) in records {
            let marker = serde_json::to_vec(&(
                record.subject_node_id(),
                record.observer_node_id(),
                record.schema(),
                record.record_key(),
            ))
            .map_err(storage_error)?;
            if seen.insert(digest(&marker)) {
                unseen.push((observed, record));
            }
        }
        if seen.len() > MAX_BASELINE_KEYS {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        // A distinct stream per generation avoids reusing any original or backfill cursor.
        let stream_key = digest(format!("{generation}:{peer_id}").as_bytes());
        let stream = format!(
            "tombstone-backfill-stale-{}",
            hex::encode(&stream_key[..12])
        );
        let epoch = state.source_epoch;
        let previous_snapshot = self.snapshot.clone();
        let previous_tombstones = self.tombstones.checkpoint();
        self.bind_cluster(cluster)?;
        self.ensure_receiver()?;
        let previous_receiver = self
            .receiver
            .as_ref()
            .expect("receiver initialized")
            .checkpoint()?;
        let result = (|| {
            let mut mutation = PendingRepositoryMutation {
                target_repository_id: identity.node_id().as_str().into(),
                progress_at_unix_seconds: Some(now),
                ..PendingRepositoryMutation::default()
            };
            let ready = vec![identity.node_id().as_str().to_owned()];
            for group in unseen.chunks(32) {
                let peer = state.peers.get_mut(peer_id).expect("validated source");
                let first = peer.source_next;
                let observed = group[0].0;
                if group.iter().any(|(time, _)| *time != observed) {
                    return Err(failure("source deletion page changed observation"));
                }
                let signed = CanonicalSegment::new(
                    cluster,
                    Cursor::new(identity.node_id().as_str(), epoch, &stream, first)?,
                    group.iter().map(|(_, record)| record.clone()).collect(),
                    peer.source_hash,
                    observed,
                    observed,
                )?
                .sign(signing_key)?;
                let wire = signed.wire_bytes()?;
                self.receive_wire_from_repository_inner_without_rebuild(
                    RepositoryWireRequest {
                        cluster_id: cluster,
                        identity,
                        wire: &wire,
                        now_unix_seconds: now,
                        ready_repositories: &ready,
                        local_repository_id: identity.node_id().as_str(),
                        allow_retained_anchor: false,
                        rollback_snapshot: None,
                        discard_gap_evidence_on_error: true,
                        allow_retained_sequence_gap: false,
                        retained_anchor_checkpoint: None,
                    },
                    Some(&mut mutation),
                    false,
                    true,
                )?;
                for (offset, (_, record)) in group.iter().enumerate() {
                    let cursor = ReplicaCursor::new(
                        identity.node_id().as_str(),
                        epoch,
                        &stream,
                        first
                            .checked_add(offset as u64)
                            .ok_or(RepositoryRuntimeError::StateLimitExceeded)?,
                    )?;
                    let key = ReplicaRecord::new(
                        &cursor,
                        record.subject_node_id(),
                        record.observer_node_id(),
                        record.schema().0,
                        record.schema().1,
                        record.record_key().to_vec(),
                        record.payload_bytes().to_vec(),
                    )?
                    .key();
                    state.baseline.insert(key_hash(&key)?);
                }
                peer.source_next = first
                    .checked_add(group.len() as u64)
                    .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
                peer.source_hash = Some(signed.segment_hash()?);
            }
            if state.baseline.len() > MAX_BASELINE_KEYS {
                return Err(RepositoryRuntimeError::StateLimitExceeded);
            }
            let peer = state.peers.get_mut(peer_id).expect("validated source");
            peer.source_markers = seen;
            peer.source_snapshot = Some(snapshot.to_owned());
            peer.cursor = next.map(str::to_owned);
            peer.done = next.is_none();
            self.snapshot.tombstones = self.tombstones.checkpoint();
            self.commit_stale_recovery(Some(&bytes), &state, mutation)
        })();
        if result.is_err() {
            self.restore(&previous_receiver, previous_snapshot)?;
            self.tombstones = TombstoneLedger::from_checkpoint(previous_tombstones)?;
        }
        result
    }
}
