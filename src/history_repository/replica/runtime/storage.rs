use super::*;

impl RepositoryReplicaRuntime {
    /// Migrate one bounded page of legacy segments after startup. The control snapshot advances
    /// only after the corresponding SQLite rows commit, so an interrupted page is safe to replay.
    pub(crate) fn migrate_legacy_segment_cursor_index_page(
        &mut self,
        limit: usize,
    ) -> Result<bool, RepositoryRuntimeError> {
        if !self.uses_sqlite_history() || self.snapshot.legacy_segment_cursor_index_complete {
            return Ok(true);
        }
        let rows = self
            .storage
            .repository_history_segments_missing_cursor_index(
                self.snapshot
                    .legacy_segment_cursor_index_after_id
                    .as_deref(),
                limit,
            )
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        let Some(last_id) = rows.last().map(|row| row.id.clone()) else {
            self.snapshot.legacy_segment_cursor_index_after_id = None;
            self.snapshot.legacy_segment_cursor_index_complete = true;
            self.persist_control_state()?;
            return Ok(true);
        };
        let segments = rows
            .into_iter()
            .map(StoredSegment::from_sqlite_row)
            .collect::<Result<Vec<_>, _>>()?;
        let indexed_rows = segments
            .iter()
            .map(StoredSegment::sqlite_row)
            .collect::<Result<Vec<_>, _>>()?;
        self.storage
            .upsert_repository_history_segments(&indexed_rows)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        self.snapshot.legacy_segment_cursor_index_after_id = Some(last_id);
        self.persist_control_state()?;
        Ok(false)
    }

    pub(crate) fn migrate_history_to_sqlite(&mut self) -> Result<(), RepositoryRuntimeError> {
        if !self.storage.is_sqlite() || self.snapshot.external_history {
            return Ok(());
        }
        let mutation = RepositoryReplicaMutation {
            records: self
                .snapshot
                .records
                .iter()
                .map(StoredRecord::sqlite_row)
                .collect::<Result<Vec<_>, _>>()?,
            tombstones: Vec::new(),
            segments: self
                .snapshot
                .segments
                .iter()
                .map(StoredSegment::sqlite_row)
                .collect::<Result<Vec<_>, _>>()?,
        };
        let mut migrated_snapshot = self.snapshot.clone();
        migrated_snapshot.records.clear();
        migrated_snapshot.segments.clear();
        migrated_snapshot.external_history = true;
        let bytes = serde_json::to_vec(&migrated_snapshot)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        if bytes.len() > MAX_RUNTIME_STATE_BYTES {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        let outcome = self
            .storage
            .commit_repository_replica_mutation(mutation, &bytes)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        self.storage_degraded |= outcome.maintenance_degraded;
        self.snapshot = migrated_snapshot;
        Ok(())
    }

    pub(crate) fn uses_sqlite_history(&self) -> bool {
        self.snapshot.external_history && self.storage.is_sqlite()
    }

    pub(super) fn finish_storage_write<T, E: std::fmt::Display>(
        &mut self,
        result: Result<T, E>,
    ) -> Result<T, RepositoryRuntimeError> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                if self.uses_sqlite_history() {
                    self.storage_degraded = true;
                }
                Err(RepositoryRuntimeError::Storage(error.to_string()))
            }
        }
    }

    pub(crate) fn stored_segments_page(
        &self,
        after_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<StoredSegment>, RepositoryRuntimeError> {
        if !self.uses_sqlite_history() {
            let (after_tombstone_rank, after_id) = segment_sync_cursor_parts(after_id);
            let mut segments = self
                .snapshot
                .segments
                .iter()
                .cloned()
                .map(|segment| {
                    (
                        segment_tombstone_rank(&segment),
                        segment_cursor_order(&segment),
                        segment,
                    )
                })
                .collect::<Vec<_>>();
            segments.sort_by(|(left_rank, left_order, _), (right_rank, right_order, _)| {
                (left_rank, left_order).cmp(&(right_rank, right_order))
            });
            let after_order = segments
                .iter()
                .find(|(_, _, segment)| segment.id == after_id)
                .map(|(_, order, _)| order.clone());
            let mut segments = segments
                .into_iter()
                .filter(|(rank, order, _)| {
                    after_order
                        .as_ref()
                        .map_or(*rank >= after_tombstone_rank, |after_order| {
                            (rank, order) > (&after_tombstone_rank, after_order)
                        })
                })
                .map(|(_, _, segment)| segment)
                .collect::<Vec<_>>();
            segments.truncate(limit);
            return Ok(segments);
        }
        let segments = self
            .storage
            .repository_history_segments_page(after_id, limit)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
            .into_iter()
            .map(StoredSegment::from_sqlite_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(segments)
    }

    pub(crate) fn stored_segments_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<StoredSegment>, RepositoryRuntimeError> {
        if !self.uses_sqlite_history() {
            let requested = ids.iter().collect::<BTreeSet<_>>();
            return Ok(self
                .snapshot
                .segments
                .iter()
                .filter(|segment| requested.contains(&segment.id))
                .cloned()
                .collect());
        }
        self.storage
            .repository_history_segments_by_ids(ids)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
            .into_iter()
            .map(StoredSegment::from_sqlite_row)
            .collect()
    }

    pub(crate) fn sqlite_records(
        &self,
        subject_node_id: Option<&str>,
        start_unix_seconds: Option<u64>,
        end_unix_seconds: Option<u64>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<StoredRecord>, RepositoryRuntimeError> {
        self.storage
            .repository_history_records(
                subject_node_id,
                start_unix_seconds,
                end_unix_seconds,
                offset,
                limit,
            )
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
            .into_iter()
            .map(StoredRecord::from_sqlite_row)
            .collect()
    }

    pub(crate) fn incomplete_aggregate_gap(
        &self,
        query: &HistoryQuery,
    ) -> Result<Option<(u64, u64)>, RepositoryRuntimeError> {
        if self.uses_sqlite_history() {
            return self
                .storage
                .repository_history_incomplete_aggregate_range(
                    query.subject_node_id(),
                    query.range().start_unix_seconds(),
                    query.range().end_unix_seconds(),
                )
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()));
        }
        Ok(retention::incomplete_aggregate_gap(
            self.snapshot.records.iter().filter(|record| {
                query
                    .subject_node_id()
                    .is_none_or(|subject_node_id| record.subject_node_id == subject_node_id)
                    && {
                        let (start, end) = retention::record_time_range(record);
                        start <= query.range().end_unix_seconds()
                            && query.range().start_unix_seconds() <= end
                    }
            }),
        ))
    }
    pub(crate) fn prune_retention(
        &mut self,
        now_unix_seconds: u64,
    ) -> Result<(), RepositoryRuntimeError> {
        if self.uses_sqlite_history() {
            return self.prune_sqlite_retention(now_unix_seconds);
        }
        retention::prune_records(
            &mut self.snapshot.records,
            &self.snapshot.gaps,
            now_unix_seconds,
            self.snapshot.cluster_id.as_deref(),
        );
        // Signed wire payloads are only an anti-entropy cache. The two-year contract is carried
        // by compacted canonical SQLite rows; retaining minute-sized wire payloads for that full
        // period would defeat the repository quota before tiering can help.
        let repair_cache_cutoff = now_unix_seconds.saturating_sub(
            super::super::RepositoryRetentionPolicy::default().minute_retention_seconds(),
        );
        self.snapshot
            .segments
            .retain(|segment| segment.closed_at_unix_seconds >= repair_cache_cutoff);
        Ok(())
    }

    pub(crate) fn prune_sqlite_retention(
        &mut self,
        now_unix_seconds: u64,
    ) -> Result<(), RepositoryRuntimeError> {
        if self
            .storage
            .has_active_repository_history_export(now_unix_seconds)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
        {
            return Ok(());
        }
        let policy = super::super::RepositoryRetentionPolicy::default();
        let cutoff = now_unix_seconds.saturating_sub(policy.minute_retention_seconds());
        let after = self
            .snapshot
            .retention_compaction_cursor
            .as_ref()
            .map(RepositoryHistoryCompactionCursor::from);
        let fetched_rows = self
            .storage
            .repository_history_records_for_compaction(
                cutoff,
                after.as_ref(),
                RETENTION_COMPACTION_PAGE_SIZE
                    .saturating_add(RETENTION_COMPACTION_BUCKET_LOOKAHEAD),
            )
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        let fetched_row_count = fetched_rows.len();
        if fetched_rows.is_empty() {
            self.snapshot.retention_compaction_cursor = None;
            self.snapshot.retention_compaction_continuation = None;
            return self.persist_control_state();
        }
        let mut rows = fetched_rows.clone();
        let mut has_more = rows.len()
            == RETENTION_COMPACTION_PAGE_SIZE.saturating_add(RETENTION_COMPACTION_BUCKET_LOOKAHEAD);
        if rows.len() > RETENTION_COMPACTION_PAGE_SIZE {
            let page_boundary = rows
                .last()
                .expect("nonempty retention lookahead page")
                .observed_start_unix_seconds;
            let closed_prefix_len = rows
                .iter()
                .rposition(|row| {
                    let record = StoredRecord::from_sqlite_row(row.clone())
                        .expect("SQLite compaction row was previously validated");
                    retention::compaction_bucket_end(&record, now_unix_seconds)
                        .is_none_or(|bucket_end| bucket_end < page_boundary)
                })
                .map_or(0, |index| index + 1);
            rows.truncate(closed_prefix_len);
        }
        has_more |= rows.len() < fetched_row_count;
        // A page can contain many retention buckets sharing one timestamp. Process that bounded
        // page even when no single bucket reaches the lookahead boundary; the continuation below
        // carries each aggregate independently so the keyset still advances.
        if rows.is_empty() && !fetched_rows.is_empty() {
            rows = fetched_rows.clone();
            has_more = true;
        }
        if rows.is_empty() {
            // No closed bucket fits in this bounded lookahead. Retain the cursor and wait for
            // later history rather than creating a partial aggregate or loading an unbounded
            // bucket into memory.
            return Ok(());
        }
        let continuation = self.snapshot.retention_compaction_continuation.take();
        let continuation_aggregates = continuation
            .map(|continuation| {
                if continuation.aggregates.is_empty() {
                    continuation.aggregate.into_iter().collect()
                } else {
                    continuation.aggregates
                }
            })
            .unwrap_or_default();
        let mut removed_rows = rows.clone();
        removed_rows.extend(
            continuation_aggregates
                .iter()
                .map(StoredRecord::sqlite_row)
                .collect::<Result<Vec<_>, _>>()?,
        );
        let mut records = rows
            .iter()
            .cloned()
            .map(StoredRecord::from_sqlite_row)
            .collect::<Result<Vec<_>, _>>()?;
        records.extend(continuation_aggregates);
        let should_continue = rows.len() == fetched_row_count && has_more;
        retention::prune_records(
            &mut records,
            &self.snapshot.gaps,
            now_unix_seconds,
            self.snapshot.cluster_id.as_deref(),
        );
        let retained = records
            .iter()
            .map(StoredRecord::sqlite_row)
            .collect::<Result<Vec<_>, _>>()?;
        if !rows.is_empty() {
            let result = self
                .storage
                .replace_repository_history_records(&removed_rows, &retained);
            self.finish_storage_write(result)?;
        }
        self.snapshot.retention_compaction_cursor =
            if !has_more && rows.len() < RETENTION_COMPACTION_PAGE_SIZE {
                None
            } else {
                rows.last().map(RetentionCompactionCursor::from)
            };
        if should_continue {
            let boundary = rows
                .last()
                .cloned()
                .map(StoredRecord::from_sqlite_row)
                .transpose()?;
            self.snapshot.retention_compaction_continuation =
                Some(RetentionCompactionContinuation {
                    aggregates: records
                        .iter()
                        .filter(|aggregate| {
                            boundary.as_ref().is_some_and(|boundary| {
                                retention::compaction_bucket_reaches(
                                    aggregate,
                                    boundary.observed_at_unix_seconds,
                                    now_unix_seconds,
                                )
                            })
                        })
                        .cloned()
                        .collect(),
                    aggregate: None,
                });
        }
        let result = self.storage.delete_repository_history_before(
            now_unix_seconds.saturating_sub(policy.max_age_seconds()),
            now_unix_seconds.saturating_sub(policy.minute_retention_seconds()),
        );
        self.finish_storage_write(result)?;
        self.persist_control_state()
    }

    pub(crate) fn repository_coverage(
        &self,
        subject_node_id: Option<&str>,
    ) -> Result<Option<QueryCoverage>, RepositoryRuntimeError> {
        if self.uses_sqlite_history() {
            return self
                .storage
                .repository_history_coverage(subject_node_id)
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
                .map(|coverage| {
                    Ok(QueryCoverage::new(
                        QueryRange::new(
                            coverage.observed_start_unix_seconds,
                            coverage.observed_end_unix_seconds,
                        )?,
                        QueryRange::new(
                            coverage.received_start_unix_seconds,
                            coverage.received_end_unix_seconds,
                        )?,
                    ))
                })
                .transpose();
        }
        let observed_start = self
            .snapshot
            .records
            .iter()
            .filter(|record| {
                subject_node_id
                    .is_none_or(|subject_node_id| record.subject_node_id == subject_node_id)
            })
            .map(|record| retention::record_time_range(record).0)
            .min();
        let Some(observed_start) = observed_start else {
            return Ok(None);
        };
        let observed_end = self
            .snapshot
            .records
            .iter()
            .filter(|record| {
                subject_node_id
                    .is_none_or(|subject_node_id| record.subject_node_id == subject_node_id)
            })
            .map(|record| retention::record_time_range(record).1)
            .max()
            .expect("repository coverage had a first record");
        let received_start = self
            .snapshot
            .records
            .iter()
            .filter(|record| {
                subject_node_id
                    .is_none_or(|subject_node_id| record.subject_node_id == subject_node_id)
            })
            .map(record_received_at)
            .min()
            .expect("repository coverage had a first record");
        let received_end = self
            .snapshot
            .records
            .iter()
            .filter(|record| {
                subject_node_id
                    .is_none_or(|subject_node_id| record.subject_node_id == subject_node_id)
            })
            .map(record_received_at)
            .max()
            .expect("repository coverage had a first record");
        Ok(Some(QueryCoverage::new(
            QueryRange::new(observed_start, observed_end).expect("record timestamps are ordered"),
            QueryRange::new(received_start, received_end).expect("record timestamps are ordered"),
        )))
    }

    pub(crate) fn serialized_snapshot_len(&self) -> Result<u64, RepositoryRuntimeError> {
        serde_json::to_vec(&self.snapshot)
            .map(|bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX))
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))
    }
}

fn segment_sync_cursor_parts(cursor: Option<&str>) -> (bool, &str) {
    match cursor {
        Some(cursor) if cursor.starts_with("t:") => (false, &cursor[2..]),
        Some(cursor) if cursor.starts_with("r:") => (true, &cursor[2..]),
        Some(_) | None => (false, ""),
    }
}

pub(super) fn segment_tombstone_rank(segment: &StoredSegment) -> bool {
    SignedSegment::from_wire(&segment.wire).map_or(true, |signed| {
        !signed
            .canonical()
            .records()
            .iter()
            .any(SyncRecord::is_tombstone)
    })
}

fn segment_cursor_order(segment: &StoredSegment) -> (String, u64, String, u64, String) {
    SignedSegment::from_wire(&segment.wire).map_or_else(
        |_| (String::new(), 0, String::new(), 0, segment.id.clone()),
        |signed| {
            let cursor = signed.canonical().first_cursor();
            (
                cursor.source_node_id().to_owned(),
                cursor.source_epoch(),
                cursor.stream().to_owned(),
                cursor.sequence(),
                segment.id.clone(),
            )
        },
    )
}
pub(crate) fn record_received_at(record: &StoredRecord) -> u64 {
    if record.received_at_unix_seconds == 0 {
        record.observed_at_unix_seconds
    } else {
        record.received_at_unix_seconds
    }
}
impl StoredRecord {
    pub(crate) fn from_record(
        observed_at_unix_seconds: u64,
        received_at_unix_seconds: u64,
        cursor: &ReplicaCursor,
        record: &SyncRecord,
    ) -> Self {
        let (schema_id, schema_version) = record.schema();
        Self {
            observed_at_unix_seconds,
            received_at_unix_seconds,
            source_node_id: cursor.source_node_id().to_owned(),
            source_epoch: cursor.source_epoch(),
            stream: cursor.stream().to_owned(),
            sequence: cursor.sequence(),
            subject_node_id: record.subject_node_id().to_owned(),
            observer_node_id: record.observer_node_id().to_owned(),
            schema_id: schema_id.to_owned(),
            schema_version,
            record_key: record.record_key().to_vec(),
            payload: record.payload_bytes().to_vec(),
            tombstone: record.is_tombstone(),
        }
    }

    pub(crate) fn matches_key(&self, key: &super::ReplicaRecordKey) -> bool {
        let (schema_id, schema_version) = key.schema();
        self.source_node_id == key.source_node_id()
            && self.source_epoch == key.source_epoch()
            && self.stream == key.stream()
            && self.subject_node_id == key.subject_node_id()
            && self.observer_node_id == key.observer_node_id()
            && self.schema_id == schema_id
            && self.schema_version == schema_version
            && self.record_key == key.record_key()
            && self.tombstone
    }

    pub(crate) fn matches_tombstone_key(&self, key: &ReplicaRecordKey, prefix: bool) -> bool {
        let (schema_id, schema_version) = key.schema();
        (prefix || self.stream == key.stream())
            && self.subject_node_id == key.subject_node_id()
            && self.observer_node_id == key.observer_node_id()
            && self.schema_id == schema_id
            && self.schema_version == schema_version
            && if prefix {
                self.record_key.starts_with(key.record_key())
            } else {
                self.record_key == key.record_key()
            }
    }

    pub(crate) fn sqlite_row(&self) -> Result<RepositoryHistoryRecordRow, RepositoryRuntimeError> {
        let (observed_start_unix_seconds, observed_end_unix_seconds) =
            retention::record_time_range(self);
        let aggregate_metadata = retention::aggregate_metadata(self);
        Ok(RepositoryHistoryRecordRow {
            source_node_id: self.source_node_id.clone(),
            source_epoch: self.source_epoch,
            stream: self.stream.clone(),
            sequence: self.sequence,
            subject_node_id: self.subject_node_id.clone(),
            observer_node_id: self.observer_node_id.clone(),
            schema_id: self.schema_id.clone(),
            schema_version: self.schema_version,
            record_key: self.record_key.clone(),
            tombstone: self.tombstone,
            observed_start_unix_seconds,
            observed_end_unix_seconds,
            received_at_unix_seconds: record_received_at(self),
            // Newly persisted raw rows explicitly carry complete metadata. NULL is reserved for
            // rows written before this metadata existed and is conservatively reported partial.
            aggregate_complete: Some(aggregate_metadata.is_none_or(|(complete, _, _)| complete)),
            aggregate_start_unix_seconds: aggregate_metadata.map(|(_, start, _)| start),
            aggregate_end_unix_seconds: aggregate_metadata.map(|(_, _, end)| end),
            payload: serde_json::to_vec(self)
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?,
        })
    }

    pub(crate) fn from_sqlite_row(
        row: RepositoryHistoryRecordRow,
    ) -> Result<Self, RepositoryRuntimeError> {
        serde_json::from_slice(&row.payload)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))
    }
}

impl StoredSegment {
    pub(crate) fn sqlite_row(&self) -> Result<RepositoryHistorySegmentRow, RepositoryRuntimeError> {
        let signed = SignedSegment::from_wire(&self.wire)?;
        let first_cursor = signed.canonical().first_cursor();
        Ok(RepositoryHistorySegmentRow {
            id: self.id.clone(),
            closed_at_unix_seconds: self.closed_at_unix_seconds,
            contains_tombstone: signed
                .canonical()
                .records()
                .iter()
                .any(SyncRecord::is_tombstone),
            source_node_id: first_cursor.source_node_id().to_owned(),
            source_epoch: first_cursor.source_epoch(),
            stream: first_cursor.stream().to_owned(),
            first_sequence: first_cursor.sequence(),
            payload: serde_json::to_vec(self)
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?,
        })
    }

    pub(crate) fn from_sqlite_row(
        row: RepositoryHistorySegmentRow,
    ) -> Result<Self, RepositoryRuntimeError> {
        let mut segment: Self = serde_json::from_slice(&row.payload)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        if segment.closed_at_unix_seconds == 0 {
            segment.closed_at_unix_seconds = row.closed_at_unix_seconds;
        }
        Ok(segment)
    }
}

impl From<StoredRecord> for RepositoryHistoryRecord {
    fn from(record: StoredRecord) -> Self {
        Self {
            observed_at_unix_seconds: record.observed_at_unix_seconds,
            source_node_id: record.source_node_id,
            source_epoch: record.source_epoch,
            stream: record.stream,
            sequence: record.sequence,
            subject_node_id: record.subject_node_id,
            observer_node_id: record.observer_node_id,
            schema_id: record.schema_id,
            schema_version: record.schema_version,
            record_key: record.record_key,
            payload: record.payload,
            tombstone: record.tombstone,
        }
    }
}
