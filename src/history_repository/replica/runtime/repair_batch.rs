use std::io::{Cursor as IoCursor, Read as _};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{
    RepositoryReplicaGap, RepositoryRuntimeError,
    sync::{
        MAX_REPAIR_GAPS, MAX_REPAIR_SEGMENTS, validate_replica_gaps,
        validate_unavailable_segment_ids,
    },
};
use crate::{
    history_sync::{MAX_DECOMPRESSION_EXPANSION_RATIO, MAX_RELAY_PLAINTEXT_BYTES},
    state::history_repository::{identity::RepositoryNodeIdentity, replica::ReplicaError},
};

const MAX_RELAY_BATCH_DECODED_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RepositoryReplicaSegment {
    pub(crate) identity: RepositoryNodeIdentity,
    pub(crate) wire: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RepositoryRepairBatch {
    pub(crate) segments: Vec<RepositoryReplicaSegment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) unavailable_segment_ids: Vec<String>,
    #[serde(default)]
    pub(crate) gaps: Vec<RepositoryReplicaGap>,
    #[serde(default, skip_serializing_if = "super::sync::is_false")]
    pub(crate) history_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) response_id: Option<String>,
}

pub(crate) struct RelayRepairPayload {
    pub(crate) batch: RepositoryRepairBatch,
    pub(crate) bytes: Vec<u8>,
}

impl RepositoryRepairBatch {
    pub(crate) fn frame_sized_relay_payload(
        self,
    ) -> Result<RelayRepairPayload, RepositoryRuntimeError> {
        if self.segments.len() > MAX_REPAIR_SEGMENTS
            || self.unavailable_segment_ids.len() > MAX_REPAIR_SEGMENTS
            || self.gaps.len() > MAX_REPAIR_GAPS
        {
            return Err(ReplicaError::RepairLimitExceeded.into());
        }
        validate_unavailable_segment_ids(&self.unavailable_segment_ids)?;
        validate_replica_gaps(&self.gaps)?;

        let gaps = self.gaps;
        let unavailable_segment_ids = self.unavailable_segment_ids;
        let history_truncated = self.history_truncated;
        let mut selected = Vec::new();
        let mut bytes = encode_relay_repair_batch(&RepositoryRepairBatch {
            segments: Vec::new(),
            unavailable_segment_ids: unavailable_segment_ids.clone(),
            gaps: gaps.clone(),
            history_truncated,
            response_id: None,
        })?;
        if bytes.len() > MAX_RELAY_PLAINTEXT_BYTES {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }

        let mut selected_wire_bytes = 0usize;
        for segment in self.segments {
            let next_wire_bytes = selected_wire_bytes.saturating_add(segment.wire.len());
            if !selected.is_empty() && next_wire_bytes > MAX_RELAY_PLAINTEXT_BYTES {
                break;
            }
            let mut candidate = selected.clone();
            candidate.push(segment);
            let candidate_batch = RepositoryRepairBatch {
                segments: candidate,
                unavailable_segment_ids: unavailable_segment_ids.clone(),
                gaps: gaps.clone(),
                history_truncated,
                response_id: None,
            };
            let candidate_bytes = encode_relay_repair_batch(&candidate_batch)?;
            if candidate_bytes.len() > MAX_RELAY_PLAINTEXT_BYTES {
                if selected.is_empty() {
                    return Err(RepositoryRuntimeError::StateLimitExceeded);
                }
                break;
            }
            selected = candidate_batch.segments;
            selected_wire_bytes = next_wire_bytes;
            bytes = candidate_bytes;
        }

        Ok(RelayRepairPayload {
            batch: RepositoryRepairBatch {
                segments: selected,
                unavailable_segment_ids,
                gaps,
                history_truncated,
                response_id: None,
            },
            bytes,
        })
    }

    pub(crate) fn from_relay_payload(payload: &[u8]) -> Result<Self, RepositoryRuntimeError> {
        if payload.len() > MAX_RELAY_PLAINTEXT_BYTES {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        let mut decoder =
            zstd::stream::read::Decoder::new(IoCursor::new(payload)).map_err(|_| {
                RepositoryRuntimeError::Storage("relay payload is malformed".to_owned())
            })?;
        let mut decoded = Vec::with_capacity(payload.len());
        let mut chunk = [0_u8; 8 * 1024];
        let max_expanded_len = payload
            .len()
            .saturating_mul(MAX_DECOMPRESSION_EXPANSION_RATIO)
            .min(MAX_RELAY_BATCH_DECODED_BYTES);
        loop {
            let read = decoder.read(&mut chunk).map_err(|_| {
                RepositoryRuntimeError::Storage("relay payload is malformed".to_owned())
            })?;
            if read == 0 {
                break;
            }
            if decoded.len().saturating_add(read) > max_expanded_len {
                return Err(RepositoryRuntimeError::StateLimitExceeded);
            }
            decoded.extend_from_slice(&chunk[..read]);
        }
        let batch = serde_json::from_slice::<Self>(&decoded).map_err(|_| {
            RepositoryRuntimeError::Storage("relay payload is malformed".to_owned())
        })?;
        if batch.segments.len() > MAX_REPAIR_SEGMENTS
            || batch.unavailable_segment_ids.len() > MAX_REPAIR_SEGMENTS
            || batch.gaps.len() > MAX_REPAIR_GAPS
        {
            return Err(ReplicaError::RepairLimitExceeded.into());
        }
        validate_unavailable_segment_ids(&batch.unavailable_segment_ids)?;
        validate_replica_gaps(&batch.gaps)?;
        Ok(batch)
    }

    pub(crate) fn response_id_digest(&self) -> Result<String, RepositoryRuntimeError> {
        let response = (
            self.segments
                .iter()
                .map(|segment| Sha256::digest(&segment.wire).to_vec())
                .collect::<Vec<_>>(),
            &self.unavailable_segment_ids,
            &self.gaps,
            self.history_truncated,
        );
        let bytes = serde_json::to_vec(&response)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        Ok(hex::encode(Sha256::digest(bytes)))
    }
}

fn encode_relay_repair_batch(
    batch: &RepositoryRepairBatch,
) -> Result<Vec<u8>, RepositoryRuntimeError> {
    let serialized = serde_json::to_vec(batch)
        .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
    zstd::stream::encode_all(IoCursor::new(serialized), 1)
        .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))
}
