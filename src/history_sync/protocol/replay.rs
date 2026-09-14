use super::{Acceptance, Acknowledgement, Cursor, ProtocolError, SignedSegment, StreamProgress};
use crate::state::history_repository::identity::RepositoryNodeIdentity;

impl super::SegmentReceiver {
    /// Return an idempotent acknowledgement for a segment already committed by the repository.
    ///
    /// A retained anchor deliberately leaves its hash chain unverified. In that state the normal
    /// replay path must continue to reject an unknown segment as a possible fork, but an exact
    /// segment whose durable ID was already committed cannot introduce new records and is safe to
    /// acknowledge. The caller proves the durable ID before invoking this method.
    pub(crate) fn accept_persisted_duplicate(
        &self,
        segment: &SignedSegment,
        identity: &RepositoryNodeIdentity,
    ) -> Result<Option<Acceptance>, ProtocolError> {
        segment.verify_identity(identity)?;
        if self.expected_cluster_id != segment.canonical.cluster_id {
            return Err(ProtocolError::ClusterMismatch);
        }
        let first = segment.canonical.first_cursor();
        let Some(progress) = self.streams.get(&first.stream_key()) else {
            return Ok(None);
        };
        if progress.epoch != first.source_epoch
            || segment.canonical.last_cursor.sequence > progress.last_sequence
        {
            return Ok(None);
        }
        Ok(Some(Acceptance::Duplicate {
            acknowledgement: Acknowledgement {
                watermark: progress.watermark(first)?,
            },
        }))
    }
}

pub(super) fn is_quarantined_continuation(
    progress: &StreamProgress,
    segment: &SignedSegment,
    first: &Cursor,
) -> Result<bool, ProtocolError> {
    let expected = progress
        .last_sequence
        .checked_add(1)
        .ok_or(ProtocolError::InvalidSegment("sequence overflow"))?;
    Ok(first.sequence == expected
        && segment.canonical.previous_segment_hash == Some(progress.last_segment_hash))
}

pub(super) fn stale_replay_acceptance(
    progress: &StreamProgress,
    segment: &SignedSegment,
    first: &Cursor,
) -> Result<Option<Acceptance>, ProtocolError> {
    // A sender may crash after the receiver commits a page but before the sender persists its
    // ACK. Once the bounded recent hash window evicts that page, a complete replay that ends
    // below the current watermark is still an idempotent duplicate. Segments that overlap the
    // watermark remain fork-protected because they may contain unseen records.
    if !progress.hash_chain_verified
        || segment.canonical.last_cursor.sequence >= progress.last_sequence
    {
        return Ok(None);
    }
    Ok(Some(Acceptance::Duplicate {
        acknowledgement: Acknowledgement {
            watermark: progress.watermark(first)?,
        },
    }))
}
