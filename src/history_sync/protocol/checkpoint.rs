use super::*;

const MAX_RESTORED_STREAMS: usize = 4_096;
const MAX_RESTORED_TOMBSTONES: usize = 8_192;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SegmentReceiverCheckpoint {
    streams: Vec<CheckpointStream>,
    tombstones: BTreeSet<TombstoneKey>,
    quarantined_streams: Vec<CheckpointQuarantine>,
    forwardable_unknown_segments: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckpointStream {
    key: StreamKey,
    progress: StreamProgress,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckpointQuarantine {
    key: StreamKey,
    next_epoch: u64,
}

impl SegmentReceiver {
    pub(crate) fn continuous_watermarks(&self) -> Vec<Cursor> {
        self.streams
            .iter()
            .map(|(stream, progress)| Cursor {
                source_node_id: stream.source_node_id.clone(),
                source_epoch: progress.epoch,
                stream: stream.stream.clone(),
                sequence: progress.last_sequence,
            })
            .collect()
    }

    pub(crate) fn checkpoint(&self) -> Result<SegmentReceiverCheckpoint, ProtocolError> {
        let forwardable_unknown_segments = self
            .forwardable_unknown_segments
            .iter()
            .map(SignedSegment::wire_bytes)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(SegmentReceiverCheckpoint {
            streams: self
                .streams
                .iter()
                .map(|(key, progress)| CheckpointStream {
                    key: key.clone(),
                    progress: progress.clone(),
                })
                .collect(),
            tombstones: self.tombstones.clone(),
            quarantined_streams: self
                .quarantined_streams
                .iter()
                .map(|(key, next_epoch)| CheckpointQuarantine {
                    key: key.clone(),
                    next_epoch: *next_epoch,
                })
                .collect(),
            forwardable_unknown_segments,
        })
    }

    pub(crate) fn from_checkpoint(
        expected_cluster_id: impl Into<String>,
        schemas: SchemaCatalog,
        checkpoint: SegmentReceiverCheckpoint,
    ) -> Result<Self, ProtocolError> {
        if checkpoint.streams.len() > MAX_RESTORED_STREAMS
            || checkpoint.quarantined_streams.len() > MAX_RESTORED_STREAMS
            || checkpoint.tombstones.len() > MAX_RESTORED_TOMBSTONES
            || checkpoint.forwardable_unknown_segments.len() > MAX_UNKNOWN_FORWARD_SEGMENTS
        {
            return Err(ProtocolError::CheckpointLimit);
        }
        let forwardable_unknown_segments = checkpoint
            .forwardable_unknown_segments
            .into_iter()
            .map(|wire| SignedSegment::from_wire(&wire))
            .collect::<Result<Vec<_>, _>>()?;
        let mut streams = BTreeMap::new();
        for entry in checkpoint.streams {
            Cursor::new(
                entry.key.source_node_id.clone(),
                entry.progress.epoch,
                entry.key.stream.clone(),
                entry.progress.last_sequence,
            )?;
            if streams.insert(entry.key, entry.progress).is_some() {
                return Err(ProtocolError::CheckpointLimit);
            }
        }
        let mut quarantined_streams = BTreeMap::new();
        for entry in checkpoint.quarantined_streams {
            Cursor::new(
                entry.key.source_node_id.clone(),
                entry.next_epoch,
                entry.key.stream.clone(),
                0,
            )?;
            if quarantined_streams
                .insert(entry.key, entry.next_epoch)
                .is_some()
            {
                return Err(ProtocolError::CheckpointLimit);
            }
        }
        Ok(Self {
            expected_cluster_id: expected_cluster_id.into(),
            schemas,
            streams,
            tombstones: checkpoint.tombstones,
            quarantined_streams,
            forwardable_unknown_segments,
        })
    }

    pub(crate) fn forget_tombstone(&mut self, cursor: &Cursor, record: &SyncRecord) {
        self.tombstones.remove(&TombstoneKey::new(cursor, record));
    }
}
