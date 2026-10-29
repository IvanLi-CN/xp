use sha2::{Digest as _, Sha256};

use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct LocalHistoryBackfillInFlight {
    page_cursor: Option<String>,
    completed: bool,
    segment_ids: Vec<String>,
    #[serde(default)]
    segments: Vec<StoredSegment>,
    #[serde(default)]
    epoch: u64,
    #[serde(default)]
    stream_state: LocalHistoryBackfillStreamState,
}

impl RepositoryReplicaRuntime {
    pub(super) fn ensure_no_local_backfill_inflight_for_rebuild(
        &self,
    ) -> Result<(), RepositoryRuntimeError> {
        if self.snapshot.local_history_backfill_inflight.is_some() {
            return Err(RepositoryRuntimeError::Storage(
                "stale repository rebuild blocked by an unacknowledged local history backfill page"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    /// Replaying a pending page cannot establish freshness after deletion evidence expires.
    /// Keep the signed chain and its checkpoint intact until a tombstone-safe rebuild can be
    /// proved; applying at an old timestamp or accepting a new retained anchor is not recovery.
    pub(super) fn ensure_local_history_backfill_is_fresh(
        &self,
        now_unix_seconds: u64,
    ) -> Result<(), RepositoryRuntimeError> {
        if self
            .snapshot
            .last_verified_unix_seconds
            .is_some_and(|last_verified| {
                ReplicaFreshness::new(last_verified, TOMBSTONE_HORIZON_SECONDS)
                    .requires_rebuild(now_unix_seconds)
            })
        {
            return Err(RepositoryRuntimeError::Storage(
                concat!(
                    "stale local history backfill requires a tombstone-safe rebuild; ",
                    "pending signed wire and chain checkpoints must be preserved"
                )
                .to_owned(),
            ));
        }
        Ok(())
    }

    fn stream_for_record(
        record: &SyncRecord,
        source_node_id: &str,
    ) -> Result<String, RepositoryRuntimeError> {
        let stream = if record.is_tombstone() {
            "tombstone"
        } else {
            source_stream_for_schema(record.schema().0).ok_or_else(|| {
                RepositoryRuntimeError::Storage(format!(
                    "history source schema has no independent stream: {}",
                    record.schema().0
                ))
            })?
        };
        Ok(format!("{stream}-backfill-{source_node_id}"))
    }

    fn epoch_for_backfill(
        &self,
        cluster_id: &str,
        source_node_id: &str,
    ) -> Result<u64, RepositoryRuntimeError> {
        if let Some(existing_node_id) = self.snapshot.local_source.node_id()
            && existing_node_id != source_node_id
        {
            return Err(RepositoryRuntimeError::Storage(
                "local history backfill identity does not match the live source".to_owned(),
            ));
        }
        Ok(if self.snapshot.local_history_backfill_epoch != 0 {
            self.snapshot.local_history_backfill_epoch
        } else if self.snapshot.local_source.epoch() != 0 {
            self.snapshot.local_source.epoch()
        } else {
            source_epoch(cluster_id, source_node_id)
        })
    }

    fn ensure_live_source_epoch_for_backfill(
        &mut self,
        cluster_id: &str,
        source_node_id: &str,
    ) -> Result<(), RepositoryRuntimeError> {
        if let Some(existing_node_id) = self.snapshot.local_source.node_id()
            && existing_node_id != source_node_id
        {
            return Err(RepositoryRuntimeError::Storage(
                "local history backfill identity does not match the live source".to_owned(),
            ));
        }
        if self.snapshot.local_source.epoch() != 0 {
            return Ok(());
        }

        let previous_snapshot = self.snapshot.clone();
        let epoch = if self.snapshot.local_history_backfill_epoch != 0 {
            // A predecessor-format page may already have pinned the backfill chain before the
            // live source snapshot was written. Preserve that chain across the repair.
            self.snapshot.local_history_backfill_epoch
        } else {
            let result = self.storage.allocate_repository_source_epoch(
                cluster_id,
                source_node_id,
                source_epoch(cluster_id, source_node_id),
            );
            self.finish_storage_write(result)?
        };
        self.set_local_source_epoch(epoch, source_node_id);
        if let Err(error) = self.persist_control_state() {
            self.snapshot = previous_snapshot;
            return Err(error);
        }
        Ok(())
    }

    fn build_segments(
        &self,
        cluster_id: &str,
        identity: &RepositoryNodeIdentity,
        signing_key: &ed25519_dalek::SigningKey,
        batches: &[(Vec<SyncRecord>, u64)],
    ) -> Result<
        (
            Vec<RepositoryReplicaSegment>,
            u64,
            LocalHistoryBackfillStreamState,
            LocalHistoryBackfillStreamState,
        ),
        RepositoryRuntimeError,
    > {
        let source_node_id = identity.node_id().as_str();
        let epoch = self.epoch_for_backfill(cluster_id, source_node_id)?;
        let previous_stream_state = self.snapshot.local_history_backfill_stream_state.clone();
        let mut stream_state = previous_stream_state.clone();
        let mut segments = Vec::new();
        for (records, observed_at_unix_seconds) in batches {
            if records.is_empty() {
                continue;
            }
            let stream = Self::stream_for_record(&records[0], source_node_id)?;
            if records.iter().any(|record| {
                Self::stream_for_record(record, source_node_id)
                    .map(|record_stream| record_stream != stream)
                    .unwrap_or(true)
            }) {
                return Err(RepositoryRuntimeError::Storage(
                    "local history backfill page mixes independent streams".to_owned(),
                ));
            }
            let (next_sequence, previous_segment_hash) =
                stream_state.get(&stream).copied().unwrap_or((0, None));
            let first_cursor = Cursor::new(source_node_id, epoch, stream.clone(), next_sequence)?;
            let signed = CanonicalSegment::new(
                cluster_id,
                first_cursor,
                records.clone(),
                previous_segment_hash,
                *observed_at_unix_seconds,
                *observed_at_unix_seconds,
            )?
            .sign(signing_key)?;
            let wire = signed.wire_bytes()?;
            let record_count = u64::try_from(records.len())
                .map_err(|_| RepositoryRuntimeError::StateLimitExceeded)?;
            let next_sequence = next_sequence
                .checked_add(record_count)
                .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
            stream_state.insert(stream, (next_sequence, Some(signed.segment_hash()?)));
            segments.push(RepositoryReplicaSegment {
                identity: identity.clone(),
                wire,
            });
        }
        Ok((segments, epoch, previous_stream_state, stream_state))
    }

    /// Persist and atomically apply one local history page. The page uses a zero-based stream
    /// family separate from the live source streams, so pending source delivery cannot alter its
    /// cursor chain and the source outbox remains untouched. The exact signed wire set is durable
    /// before record application, allowing a retry to replay the same page after interruption.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_local_history_backfill_page(
        &mut self,
        cluster_id: &str,
        identity: RepositoryNodeIdentity,
        signing_key: &ed25519_dalek::SigningKey,
        page_cursor: Option<String>,
        completed: bool,
        batches: Vec<(Vec<SyncRecord>, u64)>,
        now_unix_seconds: u64,
        ready_repositories: &[String],
        local_repository_id: &str,
    ) -> Result<(), RepositoryRuntimeError> {
        self.ensure_local_history_backfill_is_fresh(now_unix_seconds)?;
        let created = self.queue_local_history_backfill_batches(
            cluster_id,
            identity,
            signing_key,
            page_cursor,
            completed,
            batches,
        )?;
        if created.is_empty() {
            return Ok(());
        }
        self.apply_local_history_backfill_inflight_page(
            cluster_id,
            &created[0].identity,
            now_unix_seconds,
            ready_repositories,
            local_repository_id,
        )
    }

    pub(crate) fn prepare_local_history_backfill(
        &mut self,
        now_unix_seconds: u64,
    ) -> Result<(), RepositoryRuntimeError> {
        self.rebuild_if_stale(now_unix_seconds)
    }

    pub(crate) fn local_history_backfill_collection_state(
        &self,
    ) -> (Option<String>, bool, Option<u64>, u64) {
        (
            self.snapshot.local_history_backfill_cursor.clone(),
            self.snapshot.local_history_backfill_completed,
            self.snapshot.last_verified_unix_seconds,
            self.snapshot.local_source.epoch(),
        )
    }

    pub(crate) fn validate_local_history_backfill_collection_state(
        &self,
        expected: &(Option<String>, bool, Option<u64>, u64),
    ) -> Result<(), RepositoryRuntimeError> {
        if self.snapshot.local_history_backfill_inflight.is_some()
            || self.local_history_backfill_collection_state() != *expected
        {
            return Err(RepositoryRuntimeError::Storage(
                "local history backfill state changed during page collection; retry".to_owned(),
            ));
        }
        Ok(())
    }

    pub(crate) fn local_history_backfill_inflight_is_persisted(&self) -> bool {
        self.snapshot
            .local_history_backfill_inflight
            .as_ref()
            .is_some_and(|inflight| !inflight.segments.is_empty())
    }

    pub(super) fn legacy_local_history_backfill_owns_wire(&self, wire: &[u8]) -> bool {
        let Some(inflight) = self.snapshot.local_history_backfill_inflight.as_ref() else {
            return false;
        };
        if !inflight.segments.is_empty() {
            return false;
        }
        let id = hex::encode(Sha256::digest(wire));
        inflight
            .segment_ids
            .iter()
            .any(|segment_id| segment_id == &id)
    }

    /// Apply the exact wire set persisted for a new-format page and clear its checkpoint in the
    /// same repository mutation as the imported records and page cursor.
    pub(crate) fn apply_local_history_backfill_inflight_page(
        &mut self,
        cluster_id: &str,
        source_identity: &RepositoryNodeIdentity,
        now_unix_seconds: u64,
        ready_repositories: &[String],
        local_repository_id: &str,
    ) -> Result<(), RepositoryRuntimeError> {
        self.ensure_local_history_backfill_is_fresh(now_unix_seconds)?;
        let Some(inflight) = self.snapshot.local_history_backfill_inflight.clone() else {
            return Err(RepositoryRuntimeError::Storage(
                "local history backfill has no in-flight page".to_owned(),
            ));
        };
        if inflight.segments.is_empty() {
            return Err(RepositoryRuntimeError::Storage(
                "legacy local history backfill page requires source replay".to_owned(),
            ));
        }
        let segments = self.local_history_backfill_inflight_segments()?;
        let previous_stream_state = self.snapshot.local_history_backfill_stream_state.clone();
        receive::receive_local_history_backfill_page(
            self,
            cluster_id,
            source_identity,
            &segments,
            inflight.epoch,
            previous_stream_state,
            inflight.stream_state,
            inflight.page_cursor,
            inflight.completed,
            now_unix_seconds,
            ready_repositories,
            local_repository_id,
            true,
        )
    }

    /// Queue one historical backfill page without returning unrelated live outbox fronts. The
    /// caller can atomically acknowledge these exact segments with the page checkpoint.
    pub(crate) fn queue_local_history_backfill_segments(
        &mut self,
        cluster_id: &str,
        identity: RepositoryNodeIdentity,
        signing_key: &ed25519_dalek::SigningKey,
        records: Vec<SyncRecord>,
        now_unix_seconds: u64,
    ) -> Result<Vec<RepositoryReplicaSegment>, RepositoryRuntimeError> {
        let page_cursor = self.snapshot.local_history_backfill_cursor.clone();
        self.queue_local_history_backfill_batches(
            cluster_id,
            identity,
            signing_key,
            page_cursor,
            false,
            vec![(records, now_unix_seconds)],
        )
    }

    pub(crate) fn queue_local_history_backfill_batches(
        &mut self,
        cluster_id: &str,
        identity: RepositoryNodeIdentity,
        signing_key: &ed25519_dalek::SigningKey,
        page_cursor: Option<String>,
        completed: bool,
        batches: Vec<(Vec<SyncRecord>, u64)>,
    ) -> Result<Vec<RepositoryReplicaSegment>, RepositoryRuntimeError> {
        if self.snapshot.local_history_backfill_inflight.is_some() {
            return Err(RepositoryRuntimeError::Storage(
                "local history backfill already has an in-flight page".to_owned(),
            ));
        }
        if batches.iter().any(|(records, _)| !records.is_empty()) {
            self.ensure_live_source_epoch_for_backfill(cluster_id, identity.node_id().as_str())?;
        }
        let (created, epoch, _, stream_state) =
            self.build_segments(cluster_id, &identity, signing_key, &batches)?;
        if created.is_empty() {
            self.snapshot.local_history_backfill_cursor = page_cursor;
            self.snapshot.local_history_backfill_completed = completed;
            self.persist_control_state()?;
            return Ok(created);
        }
        let previous_snapshot = self.snapshot.clone();
        let previous_tombstones = self.tombstones.checkpoint();
        let stored_segments = created
            .iter()
            .map(|segment| {
                let signed = SignedSegment::from_wire(&segment.wire)?;
                Ok(StoredSegment {
                    id: hex::encode(Sha256::digest(&segment.wire)),
                    closed_at_unix_seconds: signed.canonical().closed_at_unix_seconds(),
                    identity: segment.identity.clone(),
                    wire: segment.wire.clone(),
                })
            })
            .collect::<Result<Vec<_>, RepositoryRuntimeError>>()?;
        self.snapshot.local_history_backfill_inflight = Some(LocalHistoryBackfillInFlight {
            page_cursor,
            completed,
            segment_ids: created
                .iter()
                .map(|segment| hex::encode(Sha256::digest(&segment.wire)))
                .collect(),
            segments: stored_segments,
            epoch,
            stream_state,
        });
        if let Err(error) = self.persist_control_state() {
            self.snapshot = previous_snapshot;
            self.tombstones = TombstoneLedger::from_checkpoint(previous_tombstones)?;
            return Err(error);
        }
        Ok(created)
    }

    /// Persist acknowledgement and the local historical-export cursor together. Replaying an
    /// already accepted segment is safe after a crash; moving only one of these checkpoints is
    /// not, because a new source sequence would otherwise be assigned to the same history row.
    pub(crate) fn acknowledge_local_source_segment_and_checkpoint_backfill(
        &mut self,
        delivered_wire: &[u8],
        page_cursor: Option<String>,
        completed: bool,
    ) -> Result<(), RepositoryRuntimeError> {
        if let Some(inflight) = self.snapshot.local_history_backfill_inflight.clone()
            && !inflight.segments.is_empty()
        {
            let delivered_id = hex::encode(Sha256::digest(delivered_wire));
            if inflight.segment_ids != [delivered_id]
                || inflight.page_cursor != page_cursor
                || inflight.completed != completed
            {
                return Err(RepositoryRuntimeError::Storage(
                    "local history backfill acknowledgement page is incomplete".to_owned(),
                ));
            }
            let previous_snapshot = self.snapshot.clone();
            self.snapshot.local_history_backfill_epoch = inflight.epoch;
            self.snapshot.local_history_backfill_stream_state = inflight.stream_state;
            self.snapshot.local_history_backfill_cursor = page_cursor;
            self.snapshot.local_history_backfill_completed = completed;
            self.snapshot.local_history_backfill_inflight = None;
            if let Err(error) = self.persist_control_state() {
                self.snapshot = previous_snapshot;
                return Err(error);
            }
            return Ok(());
        }
        let previous_snapshot = self.snapshot.clone();
        if !self.remove_local_source_pending_segment(delivered_wire)? {
            return Err(RepositoryRuntimeError::Storage(
                "local history backfill acknowledgement was not pending".to_owned(),
            ));
        }
        self.snapshot.local_history_backfill_inflight = None;
        self.snapshot.local_history_backfill_cursor = page_cursor;
        self.snapshot.local_history_backfill_completed = completed;
        let ids = [hex::encode(Sha256::digest(delivered_wire))];
        self.commit_legacy_local_history_backfill_ack(previous_snapshot, &ids)
    }

    pub(crate) fn local_history_backfill_inflight_checkpoint(
        &self,
    ) -> Option<(Option<String>, bool)> {
        self.snapshot
            .local_history_backfill_inflight
            .as_ref()
            .map(|inflight| (inflight.page_cursor.clone(), inflight.completed))
    }

    pub(crate) fn local_history_backfill_inflight_segments(
        &self,
    ) -> Result<Vec<RepositoryReplicaSegment>, RepositoryRuntimeError> {
        let Some(inflight) = &self.snapshot.local_history_backfill_inflight else {
            return Ok(Vec::new());
        };
        if !inflight.segments.is_empty() {
            if inflight.segment_ids.is_empty()
                || inflight.segment_ids.len() != inflight.segments.len()
            {
                return Err(RepositoryRuntimeError::Storage(
                    "local history backfill in-flight segment index is incomplete".to_owned(),
                ));
            }
            let mut seen_ids = BTreeSet::new();
            return inflight
                .segment_ids
                .iter()
                .map(|segment_id| {
                    if !seen_ids.insert(segment_id) {
                        return Err(RepositoryRuntimeError::Storage(
                            "local history backfill in-flight segment index is duplicated"
                                .to_owned(),
                        ));
                    }
                    inflight
                        .segments
                        .iter()
                        .find(|segment| &segment.id == segment_id)
                        .map(|segment| RepositoryReplicaSegment {
                            identity: segment.identity.clone(),
                            wire: segment.wire.clone(),
                        })
                        .ok_or_else(|| {
                            RepositoryRuntimeError::Storage(
                                "local history backfill segment is missing".to_owned(),
                            )
                        })
                        .and_then(|segment| {
                            let digest = hex::encode(Sha256::digest(&segment.wire));
                            if digest != *segment_id {
                                return Err(RepositoryRuntimeError::Storage(
                                    "local history backfill segment digest does not match its ID"
                                        .to_owned(),
                                ));
                            }
                            Ok(segment)
                        })
                })
                .collect();
        }
        inflight
            .segment_ids
            .iter()
            .map(|segment_id| {
                self.snapshot
                    .local_source
                    .pending_segment_by_id(segment_id)
                    .map(|segment| RepositoryReplicaSegment {
                        identity: segment.identity.clone(),
                        wire: segment.wire.clone(),
                    })
                    .ok_or_else(|| {
                        RepositoryRuntimeError::Storage(
                            "local history backfill segment is missing".to_owned(),
                        )
                    })
            })
            .collect()
    }

    fn commit_legacy_local_history_backfill_ack(
        &mut self,
        previous_snapshot: RepositoryReplicaSnapshot,
        ids: &[String],
    ) -> Result<(), RepositoryRuntimeError> {
        if self.storage.is_sqlite() {
            self.snapshot.tombstones = self.tombstones.checkpoint();
            let control_payload = serde_json::to_vec(&self.snapshot_for_persistence())
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
            if control_payload.len() > super::MAX_RUNTIME_STATE_BYTES {
                self.snapshot = previous_snapshot;
                return Err(RepositoryRuntimeError::StateLimitExceeded);
            }
            if let Err(error) = self
                .storage
                .acknowledge_source_delivery_journal_with_cursor_and_control(
                    ids,
                    None,
                    None,
                    None,
                    &control_payload,
                )
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))
            {
                self.snapshot = previous_snapshot;
                return Err(error);
            }
            self.hydrate_source_delivery_journal()?;
        } else if let Err(error) = self.persist_control_state() {
            self.snapshot = previous_snapshot;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn acknowledge_local_source_segments_and_checkpoint_backfill(
        &mut self,
        delivered_segments: &[RepositoryReplicaSegment],
        page_cursor: Option<String>,
        completed: bool,
    ) -> Result<(), RepositoryRuntimeError> {
        if let Some(inflight) = self.snapshot.local_history_backfill_inflight.clone()
            && !inflight.segments.is_empty()
        {
            let delivered_ids = delivered_segments
                .iter()
                .map(|segment| hex::encode(Sha256::digest(&segment.wire)))
                .collect::<Vec<_>>();
            if delivered_ids != inflight.segment_ids
                || inflight.page_cursor != page_cursor
                || inflight.completed != completed
            {
                return Err(RepositoryRuntimeError::Storage(
                    "local history backfill acknowledgement page is incomplete".to_owned(),
                ));
            }
            let previous_snapshot = self.snapshot.clone();
            self.snapshot.local_history_backfill_epoch = inflight.epoch;
            self.snapshot.local_history_backfill_stream_state = inflight.stream_state;
            self.snapshot.local_history_backfill_cursor = page_cursor;
            self.snapshot.local_history_backfill_completed = completed;
            self.snapshot.local_history_backfill_inflight = None;
            if let Err(error) = self.persist_control_state() {
                self.snapshot = previous_snapshot;
                return Err(error);
            }
            return Ok(());
        }
        let previous_snapshot = self.snapshot.clone();
        let Some(inflight) = self.snapshot.local_history_backfill_inflight.take() else {
            return Err(RepositoryRuntimeError::Storage(
                "local history backfill acknowledgement has no inflight page".to_owned(),
            ));
        };
        let delivered_ids = delivered_segments
            .iter()
            .map(|segment| hex::encode(Sha256::digest(&segment.wire)))
            .collect::<Vec<_>>();
        if delivered_ids != inflight.segment_ids {
            self.snapshot = previous_snapshot;
            return Err(RepositoryRuntimeError::Storage(
                "local history backfill acknowledgement page is incomplete".to_owned(),
            ));
        }
        for segment in delivered_segments {
            match self.remove_local_source_pending_segment(&segment.wire) {
                Ok(true) => {}
                Ok(false) => {
                    self.snapshot = previous_snapshot;
                    return Err(RepositoryRuntimeError::Storage(
                        "local history backfill acknowledgement was not pending".to_owned(),
                    ));
                }
                Err(error) => {
                    self.snapshot = previous_snapshot;
                    return Err(error);
                }
            }
        }
        self.snapshot.local_history_backfill_cursor = page_cursor;
        self.snapshot.local_history_backfill_completed = completed;
        let ids = delivered_segments
            .iter()
            .map(|segment| hex::encode(Sha256::digest(&segment.wire)))
            .collect::<Vec<_>>();
        self.commit_legacy_local_history_backfill_ack(previous_snapshot, &ids)
    }
}
