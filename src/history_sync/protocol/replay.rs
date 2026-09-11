use super::{Acceptance, Acknowledgement, Cursor, ProtocolError, SignedSegment, StreamProgress};

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
