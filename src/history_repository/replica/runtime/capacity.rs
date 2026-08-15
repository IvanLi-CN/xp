use sha2::{Digest as _, Sha256};

use crate::{
    history_sync::{CanonicalSegment, CursorGap, SignedSegment},
    state::history_repository::identity::RepositoryNodeIdentity,
};

use super::{
    PendingRepositoryMutation, RepositoryReplicaRuntime, RepositoryRuntimeError, StoredGap,
    StoredSegment,
};

impl RepositoryReplicaRuntime {
    pub(super) fn store_segment(
        &mut self,
        identity: &RepositoryNodeIdentity,
        wire: &[u8],
        mutation: &mut PendingRepositoryMutation,
    ) -> Result<(), RepositoryRuntimeError> {
        let id = hex::encode(Sha256::digest(wire));
        let segment = StoredSegment {
            id,
            closed_at_unix_seconds: SignedSegment::from_wire(wire)
                .map_err(RepositoryRuntimeError::from)?
                .canonical()
                .closed_at_unix_seconds(),
            identity: identity.clone(),
            wire: wire.to_vec(),
        };
        if self.uses_sqlite_history() {
            mutation.segments.push(segment.sqlite_row()?);
            return Ok(());
        }
        if !self
            .snapshot
            .segments
            .iter()
            .any(|existing| existing.id == segment.id)
        {
            self.snapshot.segments.push(segment);
        }
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

    pub(super) fn record_sequence_gap(
        &mut self,
        segment: &CanonicalSegment,
        expected: u64,
        actual: u64,
    ) {
        if actual <= expected || self.snapshot.gaps.len() == 64 {
            self.snapshot.history_truncated = true;
            return;
        }
        self.snapshot.gaps.push(StoredGap {
            source_node_id: segment.first_cursor().source_node_id().to_owned(),
            source_epoch: segment.first_cursor().source_epoch(),
            stream: segment.first_cursor().stream().to_owned(),
            first_sequence: expected,
            last_sequence: actual - 1,
            // A rejected segment cannot prove when the absent records were observed. Cover the
            // full preceding timeline so a narrow query cannot be reported complete by mistake.
            start_unix_seconds: 0,
            end_unix_seconds: segment.opened_at_unix_seconds(),
            permanent: false,
        });
    }

    pub(super) fn record_epoch_rotation_gap(
        &mut self,
        gap: &CursorGap,
        earliest_observed_unix_seconds: u64,
    ) {
        if self.snapshot.gaps.len() == 64 {
            self.snapshot.history_truncated = true;
            return;
        }
        self.snapshot.gaps.push(StoredGap {
            source_node_id: gap.requested().source_node_id().to_owned(),
            source_epoch: gap.requested().source_epoch(),
            stream: gap.requested().stream().to_owned(),
            first_sequence: gap.requested().sequence().saturating_add(1),
            last_sequence: u64::MAX,
            start_unix_seconds: 0,
            end_unix_seconds: earliest_observed_unix_seconds.saturating_sub(1),
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
