use sha2::{Digest as _, Sha256};

use crate::{
    history_sync::{CanonicalSegment, SignedSegment},
    state::history_repository::identity::RepositoryNodeIdentity,
};

use super::{
    MAX_REPOSITORY_RECORDS, MAX_REPOSITORY_SEGMENTS, RepositoryReplicaRuntime,
    RepositoryRuntimeError, StoredGap, StoredRecord, StoredSegment,
};

impl RepositoryReplicaRuntime {
    pub(super) fn store_segment(
        &mut self,
        identity: &RepositoryNodeIdentity,
        wire: &[u8],
    ) -> Result<(), RepositoryRuntimeError> {
        let id = hex::encode(Sha256::digest(wire));
        if self
            .snapshot
            .segments
            .iter()
            .any(|segment| segment.id == id)
        {
            return Ok(());
        }
        if self.snapshot.segments.len() == MAX_REPOSITORY_SEGMENTS {
            self.evict_oldest_segment();
        }
        self.snapshot.segments.push(StoredSegment {
            id,
            identity: identity.clone(),
            wire: wire.to_vec(),
        });
        Ok(())
    }

    pub(super) fn record_gap(&mut self, segment: &CanonicalSegment, permanent: bool) {
        if self.snapshot.gaps.len() == 64 {
            self.snapshot.history_truncated = true;
            return;
        }
        self.snapshot.gaps.push(StoredGap {
            source_node_id: segment.first_cursor().source_node_id().to_owned(),
            source_epoch: segment.first_cursor().source_epoch(),
            stream: segment.first_cursor().stream().to_owned(),
            first_sequence: segment.first_cursor().sequence(),
            last_sequence: segment.last_cursor().sequence(),
            start_unix_seconds: segment.opened_at_unix_seconds(),
            end_unix_seconds: segment.closed_at_unix_seconds(),
            permanent,
        });
    }

    pub(super) fn evict_oldest_record_if_needed(&mut self) {
        if self.snapshot.records.len() < MAX_REPOSITORY_RECORDS {
            return;
        }
        self.evict_oldest_record();
    }

    pub(super) fn evict_oldest_record(&mut self) -> bool {
        let Some(record) = self.snapshot.records.first().cloned() else {
            return false;
        };
        self.snapshot.records.remove(0);
        self.record_stored_record_gap(record);
        true
    }

    pub(super) fn evict_oldest_segment(&mut self) -> bool {
        let Some(evicted) = self.snapshot.segments.first().cloned() else {
            return false;
        };
        self.snapshot.segments.remove(0);
        match SignedSegment::from_wire(&evicted.wire) {
            Ok(segment) => self.record_gap(segment.canonical(), true),
            Err(_) => self.snapshot.history_truncated = true,
        }
        true
    }

    fn record_stored_record_gap(&mut self, record: StoredRecord) {
        if let Some(existing) = self.snapshot.gaps.iter_mut().find(|gap| {
            gap.permanent
                && gap.source_node_id == record.source_node_id
                && gap.source_epoch == record.source_epoch
                && gap.stream == record.stream
                && record.sequence <= gap.last_sequence.saturating_add(1)
                && gap.first_sequence <= record.sequence.saturating_add(1)
        }) {
            existing.first_sequence = existing.first_sequence.min(record.sequence);
            existing.last_sequence = existing.last_sequence.max(record.sequence);
            existing.start_unix_seconds = existing
                .start_unix_seconds
                .min(record.observed_at_unix_seconds);
            existing.end_unix_seconds = existing
                .end_unix_seconds
                .max(record.observed_at_unix_seconds);
            return;
        }
        if self.snapshot.gaps.len() == 64 {
            self.snapshot.history_truncated = true;
            return;
        }
        self.snapshot.gaps.push(StoredGap {
            source_node_id: record.source_node_id,
            source_epoch: record.source_epoch,
            stream: record.stream,
            first_sequence: record.sequence,
            last_sequence: record.sequence,
            start_unix_seconds: record.observed_at_unix_seconds,
            end_unix_seconds: record.observed_at_unix_seconds,
            permanent: true,
        });
    }

    pub(super) fn record_source_received(
        &mut self,
        segment: &CanonicalSegment,
        now_unix_seconds: u64,
    ) -> Result<(), RepositoryRuntimeError> {
        let source_node_id = segment.first_cursor().source_node_id().to_owned();
        if !self
            .snapshot
            .source_last_received_unix_seconds
            .contains_key(&source_node_id)
            && self.snapshot.source_last_received_unix_seconds.len() == 4_096
        {
            // Keep the repository data plane available when source bookkeeping is full.
            // Query results expose this loss through the broad permanent truncation gap.
            self.snapshot.history_truncated = true;
            return Ok(());
        }
        self.snapshot
            .source_last_received_unix_seconds
            .insert(source_node_id, now_unix_seconds);
        Ok(())
    }
}
