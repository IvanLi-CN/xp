use std::collections::{BTreeMap, BTreeSet};

use super::{Acceptance, Acknowledgement, Cursor, ProtocolError, SignedSegment, StreamProgress};
use crate::state::history_repository::identity::RepositoryNodeIdentity;

impl super::SegmentReceiver {
    /// The actual continuous receipt tip and signed boundary hash. A retained anchor alone
    /// cannot prove that earlier ordinary records were ever received.
    pub(crate) fn continuous_watermark_with_hash(
        &self,
        cursor: &Cursor,
    ) -> Result<Option<(Cursor, [u8; 32])>, ProtocolError> {
        self.streams
            .get(&cursor.stream_key())
            .filter(|progress| progress.epoch == cursor.source_epoch)
            .map(|progress| {
                if !progress.hash_chain_verified || !progress.previous_hash_verified() {
                    return Err(ProtocolError::HashChainMismatch);
                }
                Ok((progress.watermark(cursor)?, progress.last_segment_hash))
            })
            .transpose()
    }

    /// Trusted recovery must first verify the complete original chain through this exact tip.
    /// This does not advance or rewind the receiver and cannot mint a future acknowledgement.
    pub(crate) fn accept_verified_history_duplicate(
        &self,
        segment: &SignedSegment,
        identity: &RepositoryNodeIdentity,
        tip: &Cursor,
        tip_hash: [u8; 32],
    ) -> Result<Acceptance, ProtocolError> {
        segment.verify_identity(identity)?;
        let first = segment.canonical.first_cursor();
        if self.expected_cluster_id != segment.canonical.cluster_id {
            return Err(ProtocolError::ClusterMismatch);
        }
        if first.stream_key() != tip.stream_key()
            || first.source_epoch != tip.source_epoch
            || segment.canonical.last_cursor.sequence > tip.sequence
            || self.continuous_watermark_with_hash(tip)? != Some((tip.clone(), tip_hash))
        {
            return Err(ProtocolError::HashChainMismatch);
        }
        Ok(Acceptance::Duplicate {
            acknowledgement: Acknowledgement {
                watermark: tip.clone(),
            },
        })
    }

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

    /// Accept a local-source replay after the caller has verified a contiguous page through the
    /// receiver's current watermark. The page proof makes older entries safe to acknowledge even
    /// when their short recent-hash window has already been evicted.
    pub(crate) fn accept_verified_local_source_replay(
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
        if segment.canonical.last_cursor.sequence == progress.last_sequence
            && segment.segment_hash()? != progress.last_segment_hash
        {
            return Ok(None);
        }
        Ok(Some(Acceptance::Duplicate {
            acknowledgement: Acknowledgement {
                watermark: progress.watermark(first)?,
            },
        }))
    }

    /// Prove which local-source page streams contain a contiguous replay through the current
    /// receiver watermark. The caller may then process the page in smaller atomic batches while
    /// retaining the proof for older entries whose hashes are outside the recent window.
    pub(crate) fn verified_local_source_replay_streams(
        &self,
        segments: impl IntoIterator<Item = Result<SignedSegment, ProtocolError>>,
    ) -> Result<BTreeSet<String>, ProtocolError> {
        struct ReplayProof {
            first: Cursor,
            last_sequence: u64,
            previous_hash: Option<[u8; 32]>,
            hash: [u8; 32],
        }
        // Retain only chain metadata. Decoded record payloads are released after each wire,
        // rather than keeping a second complete delivery page alive during verification.
        let mut grouped = BTreeMap::<String, Vec<ReplayProof>>::new();
        for segment in segments {
            let segment = segment?;
            let cursor = segment.canonical.first_cursor();
            let Some(progress) = self.streams.get(&cursor.stream_key()) else {
                continue;
            };
            // Future segments sort after any possible watermark anchor and cannot be part
            // of its replay prefix. Ordinary receive still verifies their original wires.
            if cursor.sequence > progress.last_sequence {
                continue;
            }
            let key = format!(
                "{}:{}:{}",
                cursor.source_node_id, cursor.source_epoch, cursor.stream
            );
            grouped.entry(key).or_default().push(ReplayProof {
                first: cursor.clone(),
                last_sequence: segment.canonical.last_cursor.sequence,
                previous_hash: segment.canonical.previous_segment_hash(),
                hash: segment.segment_hash()?,
            });
        }
        let mut verified = BTreeSet::new();
        for (key, mut stream_segments) in grouped {
            let Some(first) = stream_segments.first() else {
                continue;
            };
            let cursor = &first.first;
            let Some(progress) = self.streams.get(&cursor.stream_key()) else {
                continue;
            };
            stream_segments.sort_by_key(|segment| segment.first.sequence);
            let Some(anchor_index) = stream_segments.iter().position(|segment| {
                segment.last_sequence == progress.last_sequence
                    && segment.hash == progress.last_segment_hash
            }) else {
                continue;
            };
            let prefix = &stream_segments[..=anchor_index];
            let contiguous = prefix.windows(2).all(|pair| {
                pair[0].last_sequence.checked_add(1) == Some(pair[1].first.sequence)
                    && pair[1].previous_hash == Some(pair[0].hash)
            });
            if contiguous {
                verified.insert(key);
            }
        }
        Ok(verified)
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
