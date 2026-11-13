use super::*;

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
        key.known_sequence() == Some(self.sequence) && self.matches_tombstone_identity(key)
    }

    pub(crate) fn matches_tombstone_identity(&self, key: &super::ReplicaRecordKey) -> bool {
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
        let backfill_stream = format!("{}-backfill-{}", key.stream(), key.source_node_id());
        (prefix || self.stream == key.stream() || self.stream == backfill_stream)
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
