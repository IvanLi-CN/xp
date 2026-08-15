use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt,
};

use ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};
use prost::Message as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{encoding, proto};
use crate::state::history_repository::identity::RepositoryNodeIdentity;

mod checkpoint;
pub(crate) use checkpoint::SegmentReceiverCheckpoint;

pub(crate) const MAX_RECORDS_PER_SEGMENT: usize = 1_000;
pub(crate) const MAX_CANONICAL_SEGMENT_BYTES: usize = 192 * 1024;
pub(crate) const MAX_RESPONSE_CANONICAL_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_RESPONSE_WIRE_BYTES: usize = 256 * 1024;
const MAX_SEGMENT_DURATION_SECONDS: u64 = 60;
const MAX_UNKNOWN_FORWARD_SEGMENTS: usize =
    MAX_RESPONSE_CANONICAL_BYTES / MAX_CANONICAL_SEGMENT_BYTES;
const MAX_RECENT_SEGMENTS_PER_STREAM: usize = MAX_UNKNOWN_FORWARD_SEGMENTS;

fn validate_identifier(kind: &'static str, value: &str) -> Result<(), ProtocolError> {
    if value.trim().is_empty() {
        return Err(ProtocolError::InvalidSegment(match kind {
            "cluster id" => "cluster id must not be empty",
            "source node id" => "source node id must not be empty",
            "stream" => "stream must not be empty",
            "schema id" => "schema id must not be empty",
            "subject node id" => "subject node id must not be empty",
            "observer node id" => "observer node id must not be empty",
            _ => "identifier must not be empty",
        }));
    }
    if value != value.trim() || value.chars().any(char::is_control) {
        return Err(ProtocolError::InvalidSegment("identifier is malformed"));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Cursor {
    source_node_id: String,
    source_epoch: u64,
    stream: String,
    sequence: u64,
}

impl Cursor {
    pub(crate) fn new(
        source_node_id: impl Into<String>,
        source_epoch: u64,
        stream: impl Into<String>,
        sequence: u64,
    ) -> Result<Self, ProtocolError> {
        let source_node_id = source_node_id.into();
        let stream = stream.into();
        validate_identifier("source node id", &source_node_id)?;
        validate_identifier("stream", &stream)?;
        if source_epoch > i64::MAX as u64 || sequence > i64::MAX as u64 {
            return Err(ProtocolError::InvalidSegment(
                "cursor exceeds durable integer range",
            ));
        }
        Ok(Self {
            source_node_id,
            source_epoch,
            stream,
            sequence,
        })
    }

    pub(crate) fn source_node_id(&self) -> &str {
        &self.source_node_id
    }

    pub(crate) fn source_epoch(&self) -> u64 {
        self.source_epoch
    }

    pub(crate) fn stream(&self) -> &str {
        &self.stream
    }

    pub(crate) fn sequence(&self) -> u64 {
        self.sequence
    }

    fn with_sequence(&self, sequence: u64) -> Result<Self, ProtocolError> {
        Self::new(
            self.source_node_id.clone(),
            self.source_epoch,
            self.stream.clone(),
            sequence,
        )
    }

    fn stream_key(&self) -> StreamKey {
        StreamKey {
            source_node_id: self.source_node_id.clone(),
            stream: self.stream.clone(),
        }
    }

    fn into_proto(self) -> proto::Cursor {
        proto::Cursor {
            source_node_id: self.source_node_id,
            source_epoch: self.source_epoch,
            stream: self.stream,
            sequence: self.sequence,
        }
    }

    fn from_proto(cursor: proto::Cursor) -> Result<Self, ProtocolError> {
        Self::new(
            cursor.source_node_id,
            cursor.source_epoch,
            cursor.stream,
            cursor.sequence,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyncRecord {
    subject_node_id: String,
    observer_node_id: String,
    schema_id: String,
    schema_version: u32,
    record_key: Vec<u8>,
    payload: Vec<u8>,
    tombstone: bool,
}

impl SyncRecord {
    pub(crate) fn new(
        subject_node_id: impl Into<String>,
        observer_node_id: impl Into<String>,
        schema_id: impl Into<String>,
        schema_version: u32,
        record_key: Vec<u8>,
        payload: Vec<u8>,
        tombstone: bool,
    ) -> Self {
        Self {
            subject_node_id: subject_node_id.into(),
            observer_node_id: observer_node_id.into(),
            schema_id: schema_id.into(),
            schema_version,
            record_key,
            payload,
            tombstone,
        }
    }

    pub(crate) fn is_tombstone(&self) -> bool {
        self.tombstone
    }

    pub(crate) fn subject_node_id(&self) -> &str {
        &self.subject_node_id
    }

    pub(crate) fn observer_node_id(&self) -> &str {
        &self.observer_node_id
    }

    pub(crate) fn schema(&self) -> (&str, u32) {
        (&self.schema_id, self.schema_version)
    }

    pub(crate) fn record_key(&self) -> &[u8] {
        &self.record_key
    }

    pub(crate) fn payload_bytes(&self) -> &[u8] {
        &self.payload
    }

    fn validate(&self) -> Result<(), ProtocolError> {
        validate_identifier("subject node id", &self.subject_node_id)?;
        validate_identifier("observer node id", &self.observer_node_id)?;
        validate_identifier("schema id", &self.schema_id)?;
        if self.schema_version == 0 {
            return Err(ProtocolError::InvalidSegment(
                "schema version must be greater than zero",
            ));
        }
        if self.record_key.is_empty() {
            return Err(ProtocolError::InvalidSegment(
                "record key must not be empty",
            ));
        }
        Ok(())
    }

    fn into_proto(self) -> proto::Record {
        proto::Record {
            subject_node_id: self.subject_node_id,
            observer_node_id: self.observer_node_id,
            schema_id: self.schema_id,
            schema_version: self.schema_version,
            record_key: self.record_key,
            payload: self.payload,
            tombstone: self.tombstone,
        }
    }

    fn from_proto(record: proto::Record) -> Self {
        Self {
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

pub(crate) fn prioritize_tombstones(records: Vec<SyncRecord>) -> Vec<SyncRecord> {
    let (tombstones, remaining): (Vec<_>, Vec<_>) =
        records.into_iter().partition(SyncRecord::is_tombstone);
    tombstones.into_iter().chain(remaining).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CanonicalSegment {
    cluster_id: String,
    first_cursor: Cursor,
    last_cursor: Cursor,
    records: Vec<SyncRecord>,
    records_hash: [u8; 32],
    previous_segment_hash: Option<[u8; 32]>,
    opened_at_unix_seconds: u64,
    closed_at_unix_seconds: u64,
}

impl CanonicalSegment {
    pub(crate) fn new(
        cluster_id: impl Into<String>,
        first_cursor: Cursor,
        records: Vec<SyncRecord>,
        previous_segment_hash: Option<[u8; 32]>,
        opened_at_unix_seconds: u64,
        closed_at_unix_seconds: u64,
    ) -> Result<Self, ProtocolError> {
        let last_sequence = first_cursor
            .sequence
            .checked_add(
                u64::try_from(records.len())
                    .map_err(|_| ProtocolError::InvalidSegment("record count overflows"))?
                    .saturating_sub(1),
            )
            .ok_or(ProtocolError::InvalidSegment("sequence range overflows"))?;
        let records_hash = hash_records(&records);
        let segment = Self {
            cluster_id: cluster_id.into(),
            last_cursor: first_cursor.with_sequence(last_sequence)?,
            first_cursor,
            records,
            records_hash,
            previous_segment_hash,
            opened_at_unix_seconds,
            closed_at_unix_seconds,
        };
        segment.validate()?;
        Ok(segment)
    }

    pub(crate) fn first_cursor(&self) -> &Cursor {
        &self.first_cursor
    }

    pub(crate) fn last_cursor(&self) -> &Cursor {
        &self.last_cursor
    }

    pub(crate) fn records(&self) -> &[SyncRecord] {
        &self.records
    }

    pub(crate) fn canonical_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        Ok(self.proto_without_signature().encode_to_vec())
    }

    pub(crate) fn sign(&self, signing_key: &SigningKey) -> Result<SignedSegment, ProtocolError> {
        let canonical = self.canonical_bytes()?;
        let signature = signing_key.sign(&canonical).to_bytes();
        Ok(SignedSegment {
            canonical: self.clone(),
            signature,
        })
    }

    pub(crate) fn records_hash(&self) -> [u8; 32] {
        self.records_hash
    }

    pub(crate) fn opened_at_unix_seconds(&self) -> u64 {
        self.opened_at_unix_seconds
    }

    pub(crate) fn closed_at_unix_seconds(&self) -> u64 {
        self.closed_at_unix_seconds
    }

    fn validate(&self) -> Result<(), ProtocolError> {
        validate_identifier("cluster id", &self.cluster_id)?;
        if self.records.is_empty() {
            return Err(ProtocolError::InvalidSegment(
                "segment must contain records",
            ));
        }
        if self.records.len() > MAX_RECORDS_PER_SEGMENT {
            return Err(ProtocolError::SegmentRecordLimit {
                actual: self.records.len(),
            });
        }
        for record in &self.records {
            record.validate()?;
        }
        if self
            .records
            .windows(2)
            .any(|pair| !pair[0].tombstone && pair[1].tombstone)
        {
            return Err(ProtocolError::InvalidSegment(
                "tombstones must precede ordinary records",
            ));
        }
        if self.records_hash != hash_records(&self.records) {
            return Err(ProtocolError::InvalidSegment(
                "record hash does not match records",
            ));
        }
        if self.closed_at_unix_seconds < self.opened_at_unix_seconds
            || self.closed_at_unix_seconds - self.opened_at_unix_seconds
                > MAX_SEGMENT_DURATION_SECONDS
        {
            return Err(ProtocolError::SegmentDurationLimit);
        }
        let expected_last = self.first_cursor.sequence.checked_add(
            u64::try_from(self.records.len())
                .map_err(|_| ProtocolError::InvalidSegment("record count overflows"))?
                .saturating_sub(1),
        );
        if expected_last != Some(self.last_cursor.sequence)
            || self.first_cursor.source_node_id != self.last_cursor.source_node_id
            || self.first_cursor.source_epoch != self.last_cursor.source_epoch
            || self.first_cursor.stream != self.last_cursor.stream
        {
            return Err(ProtocolError::InvalidSegment(
                "cursor range does not match records",
            ));
        }
        let canonical_size = self.proto_without_signature().encoded_len();
        if canonical_size > MAX_CANONICAL_SEGMENT_BYTES {
            return Err(ProtocolError::SegmentCanonicalLimit {
                actual: canonical_size,
            });
        }
        Ok(())
    }

    fn proto_without_signature(&self) -> proto::Segment {
        proto::Segment {
            cluster_id: self.cluster_id.clone(),
            first_cursor: Some(self.first_cursor.clone().into_proto()),
            last_cursor: Some(self.last_cursor.clone().into_proto()),
            records: self
                .records
                .clone()
                .into_iter()
                .map(SyncRecord::into_proto)
                .collect(),
            previous_segment_hash: self
                .previous_segment_hash
                .map_or_else(Vec::new, |hash| hash.to_vec()),
            opened_at_unix_seconds: self.opened_at_unix_seconds,
            closed_at_unix_seconds: self.closed_at_unix_seconds,
            signature: Vec::new(),
            records_hash: self.records_hash.to_vec(),
        }
    }

    fn from_proto(segment: proto::Segment) -> Result<(Self, [u8; 64]), ProtocolError> {
        let signature: [u8; 64] = segment
            .signature
            .try_into()
            .map_err(|_| ProtocolError::InvalidSignature)?;
        let previous_segment_hash = match segment.previous_segment_hash.as_slice() {
            [] => None,
            hash => Some(
                hash.try_into()
                    .map_err(|_| ProtocolError::InvalidSegment("invalid previous hash length"))?,
            ),
        };
        let records_hash = segment
            .records_hash
            .try_into()
            .map_err(|_| ProtocolError::InvalidSegment("invalid records hash length"))?;
        let first_cursor = segment
            .first_cursor
            .ok_or(ProtocolError::InvalidSegment("missing first cursor"))?;
        let canonical = Self {
            cluster_id: segment.cluster_id,
            first_cursor: Cursor::from_proto(first_cursor)?,
            last_cursor: Cursor::from_proto(
                segment
                    .last_cursor
                    .ok_or(ProtocolError::InvalidSegment("missing last cursor"))?,
            )?,
            records: segment
                .records
                .into_iter()
                .map(SyncRecord::from_proto)
                .collect(),
            records_hash,
            previous_segment_hash,
            opened_at_unix_seconds: segment.opened_at_unix_seconds,
            closed_at_unix_seconds: segment.closed_at_unix_seconds,
        };
        canonical.validate()?;
        Ok((canonical, signature))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SignedSegment {
    canonical: CanonicalSegment,
    signature: [u8; 64],
}

impl SignedSegment {
    pub(crate) fn verify(&self, verifying_key: &VerifyingKey) -> Result<(), ProtocolError> {
        let bytes = self.canonical.canonical_bytes()?;
        verifying_key
            .verify_strict(&bytes, &Signature::from_bytes(&self.signature))
            .map_err(|_| ProtocolError::InvalidSignature)
    }

    pub(crate) fn canonical(&self) -> &CanonicalSegment {
        &self.canonical
    }

    pub(crate) fn verify_identity(
        &self,
        identity: &RepositoryNodeIdentity,
    ) -> Result<(), ProtocolError> {
        if self.canonical.first_cursor.source_node_id != identity.node_id().as_str() {
            return Err(ProtocolError::SourceIdentityMismatch);
        }
        let verifying_key = VerifyingKey::from_bytes(identity.ed25519_public_key().as_bytes())
            .map_err(|_| ProtocolError::InvalidSigningKey)?;
        self.verify(&verifying_key)
    }

    pub(crate) fn segment_hash(&self) -> Result<[u8; 32], ProtocolError> {
        let canonical = self.canonical.canonical_bytes()?;
        let mut hash = Sha256::new();
        hash.update(canonical);
        hash.update(self.signature);
        Ok(hash.finalize().into())
    }

    pub(crate) fn wire_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        let wire = self.proto_with_signature().encode_to_vec();
        if wire.len() > MAX_RESPONSE_WIRE_BYTES {
            return Err(ProtocolError::WireLimit { actual: wire.len() });
        }
        Ok(wire)
    }

    pub(crate) fn from_wire(wire: &[u8]) -> Result<Self, ProtocolError> {
        if wire.len() > MAX_RESPONSE_WIRE_BYTES {
            return Err(ProtocolError::WireLimit { actual: wire.len() });
        }
        let segment = proto::Segment::decode(wire).map_err(|_| ProtocolError::InvalidWire)?;
        let (canonical, signature) = CanonicalSegment::from_proto(segment)?;
        let signed = Self {
            canonical,
            signature,
        };
        if signed.wire_bytes()? != wire {
            return Err(ProtocolError::NonCanonicalWire);
        }
        Ok(signed)
    }

    fn proto_with_signature(&self) -> proto::Segment {
        let mut segment = self.canonical.proto_without_signature();
        segment.signature = self.signature.to_vec();
        segment
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PayloadEncoding {
    Identity,
    ZstandardLevel1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EncodedResponse {
    encoding: PayloadEncoding,
    wire: Vec<u8>,
    canonical_len: usize,
}

impl EncodedResponse {
    pub(crate) fn encode(canonical: Vec<u8>) -> Result<Self, ProtocolError> {
        let canonical_len = canonical.len();
        let (encoding, wire) = encoding::encode(canonical)?;
        Ok(Self {
            encoding,
            canonical_len,
            wire,
        })
    }

    pub(crate) fn from_wire(
        encoding: PayloadEncoding,
        canonical_len: usize,
        wire: Vec<u8>,
    ) -> Result<Self, ProtocolError> {
        encoding::validate_wire(encoding, canonical_len, &wire)?;
        Ok(Self {
            encoding,
            wire,
            canonical_len,
        })
    }

    pub(crate) fn encoding(&self) -> PayloadEncoding {
        self.encoding
    }

    pub(crate) fn wire(&self) -> &[u8] {
        &self.wire
    }

    pub(crate) fn decode(&self) -> Result<Vec<u8>, ProtocolError> {
        encoding::decode(self.encoding, &self.wire, self.canonical_len)
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SchemaCatalog(BTreeSet<(String, u32)>);

impl SchemaCatalog {
    pub(crate) fn new(schemas: impl IntoIterator<Item = (String, u32)>) -> Self {
        Self(schemas.into_iter().collect())
    }

    fn contains(&self, record: &SyncRecord) -> bool {
        self.0
            .contains(&(record.schema_id.clone(), record.schema_version))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Acknowledgement {
    watermark: Cursor,
}

impl Acknowledgement {
    pub(crate) fn watermark(&self) -> &Cursor {
        &self.watermark
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CursorGap {
    requested: Cursor,
    earliest_available: Cursor,
}

impl CursorGap {
    pub(crate) fn requested(&self) -> &Cursor {
        &self.requested
    }

    pub(crate) fn earliest_available(&self) -> &Cursor {
        &self.earliest_available
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CursorAvailability {
    Available,
    Expired(CursorGap),
}

pub(crate) fn cursor_availability(
    requested: Cursor,
    earliest_available: Cursor,
) -> Result<CursorAvailability, ProtocolError> {
    if requested.stream_key() != earliest_available.stream_key() {
        return Err(ProtocolError::CursorIdentityMismatch);
    }
    if requested.source_epoch < earliest_available.source_epoch
        || (requested.source_epoch == earliest_available.source_epoch
            && requested.sequence < earliest_available.sequence)
    {
        Ok(CursorAvailability::Expired(CursorGap {
            requested,
            earliest_available,
        }))
    } else {
        Ok(CursorAvailability::Available)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Acceptance {
    Accepted {
        acknowledgement: Acknowledgement,
        gap: Option<CursorGap>,
        unknown_schema_records: usize,
    },
    Duplicate {
        acknowledgement: Acknowledgement,
    },
}

impl Acceptance {
    pub(crate) fn unknown_schema_records(&self) -> usize {
        match self {
            Self::Accepted {
                unknown_schema_records,
                ..
            } => *unknown_schema_records,
            Self::Duplicate { .. } => 0,
        }
    }

    pub(crate) fn acknowledgement(&self) -> &Acknowledgement {
        match self {
            Self::Accepted {
                acknowledgement, ..
            }
            | Self::Duplicate { acknowledgement } => acknowledgement,
        }
    }

    pub(crate) fn gap(&self) -> Option<&CursorGap> {
        match self {
            Self::Accepted { gap, .. } => gap.as_ref(),
            Self::Duplicate { .. } => None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct SegmentReceiver {
    expected_cluster_id: String,
    schemas: SchemaCatalog,
    streams: BTreeMap<StreamKey, StreamProgress>,
    tombstones: BTreeSet<TombstoneKey>,
    quarantined_streams: BTreeMap<StreamKey, u64>,
    forwardable_unknown_segments: Vec<SignedSegment>,
}

impl SegmentReceiver {
    pub(crate) fn for_cluster(
        expected_cluster_id: impl Into<String>,
        schemas: SchemaCatalog,
    ) -> Self {
        Self {
            expected_cluster_id: expected_cluster_id.into(),
            schemas,
            streams: BTreeMap::new(),
            tombstones: BTreeSet::new(),
            quarantined_streams: BTreeMap::new(),
            forwardable_unknown_segments: Vec::new(),
        }
    }

    pub(crate) fn accept(
        &mut self,
        segment: &SignedSegment,
        identity: &RepositoryNodeIdentity,
    ) -> Result<Acceptance, ProtocolError> {
        segment.verify_identity(identity)?;
        if self.expected_cluster_id != segment.canonical.cluster_id {
            return Err(ProtocolError::ClusterMismatch);
        }
        let first = segment.canonical.first_cursor();
        let stream_key = first.stream_key();
        if self.quarantined_streams.contains_key(&stream_key)
            && first.source_epoch == self.streams[&stream_key].epoch
        {
            return Err(ProtocolError::Quarantined);
        }
        let segment_hash = segment.segment_hash()?;
        let mut gap = None;
        let mut rotates_epoch = false;
        if let Some(progress) = self.streams.get(&stream_key) {
            if first.source_epoch > progress.epoch {
                let expected_epoch = progress.epoch.saturating_add(1);
                if first.source_epoch != expected_epoch
                    || first.sequence != 0
                    || segment.canonical.previous_segment_hash.is_some()
                {
                    return Err(ProtocolError::EpochGap {
                        expected: expected_epoch,
                        actual: first.source_epoch,
                    });
                }
                gap = Some(CursorGap {
                    requested: Cursor::new(
                        first.source_node_id.clone(),
                        progress.epoch,
                        first.stream.clone(),
                        progress.last_sequence,
                    )?,
                    earliest_available: first.clone(),
                });
                rotates_epoch = true;
            } else if first.source_epoch < progress.epoch {
                return Err(ProtocolError::EpochGap {
                    expected: progress.epoch,
                    actual: first.source_epoch,
                });
            }
            if !rotates_epoch {
                if first.sequence <= progress.last_sequence {
                    if progress.recent_segments.iter().any(|known| {
                        known.first_sequence == first.sequence
                            && known.last_sequence == segment.canonical.last_cursor.sequence
                            && known.hash == segment_hash
                    }) {
                        return Ok(Acceptance::Duplicate {
                            acknowledgement: Acknowledgement {
                                watermark: progress.watermark(first)?,
                            },
                        });
                    }
                    let next_epoch =
                        progress
                            .epoch
                            .checked_add(1)
                            .ok_or(ProtocolError::InvalidSegment(
                                "source epoch overflows after a fork",
                            ))?;
                    Cursor::new(
                        first.source_node_id.clone(),
                        next_epoch,
                        first.stream.clone(),
                        0,
                    )?;
                    self.quarantined_streams.insert(stream_key, next_epoch);
                    return Err(ProtocolError::ForkDetected { next_epoch });
                }
                let expected = progress
                    .last_sequence
                    .checked_add(1)
                    .ok_or(ProtocolError::InvalidSegment("sequence overflow"))?;
                if first.sequence != expected {
                    return Err(ProtocolError::SequenceGap {
                        expected,
                        actual: first.sequence,
                    });
                }
                if segment.canonical.previous_segment_hash != Some(progress.last_segment_hash) {
                    return Err(ProtocolError::HashChainMismatch);
                }
            }
        } else if segment.canonical.previous_segment_hash.is_some() {
            return Err(ProtocolError::HashChainMismatch);
        }

        let records = segment.canonical.records();
        let unknown_schema_records = records
            .iter()
            .filter(|record| !self.schemas.contains(record))
            .count();
        if unknown_schema_records > 0
            && self.forwardable_unknown_segments.len() >= MAX_UNKNOWN_FORWARD_SEGMENTS
        {
            return Err(ProtocolError::UnknownSchemaBacklogFull);
        }
        let tombstone_keys: BTreeSet<_> = records
            .iter()
            .filter(|record| record.tombstone)
            .map(|record| TombstoneKey::new(first, record))
            .collect();
        let resurrected = records
            .iter()
            .filter(|record| !record.tombstone)
            .any(|record| {
                let key = TombstoneKey::new(first, record);
                self.tombstones.contains(&key) || tombstone_keys.contains(&key)
            });
        if resurrected {
            return Err(ProtocolError::ResurrectionPrevented);
        }
        self.tombstones.extend(tombstone_keys);

        if unknown_schema_records > 0 {
            self.forwardable_unknown_segments.push(segment.clone());
        }
        let mut recent_segments = if rotates_epoch {
            VecDeque::new()
        } else {
            self.streams
                .get(&first.stream_key())
                .map(|progress| progress.recent_segments.clone())
                .unwrap_or_default()
        };
        if recent_segments.len() == MAX_RECENT_SEGMENTS_PER_STREAM {
            recent_segments.pop_front();
        }
        recent_segments.push_back(SegmentHashRange {
            first_sequence: first.sequence,
            last_sequence: segment.canonical.last_cursor.sequence,
            hash: segment_hash,
        });
        self.streams.insert(
            first.stream_key(),
            StreamProgress {
                epoch: first.source_epoch,
                last_sequence: segment.canonical.last_cursor.sequence,
                last_segment_hash: segment_hash,
                recent_segments,
            },
        );
        if rotates_epoch {
            self.quarantined_streams.remove(&first.stream_key());
        }
        Ok(Acceptance::Accepted {
            acknowledgement: Acknowledgement {
                watermark: segment.canonical.last_cursor.clone(),
            },
            gap,
            unknown_schema_records,
        })
    }

    pub(crate) fn advance_declared_sequence_gap(
        &mut self,
        next: &Cursor,
        first_missing: u64,
        last_missing: u64,
    ) -> Result<bool, ProtocolError> {
        let stream_key = next.stream_key();
        let Some(progress) = self.streams.get_mut(&stream_key) else {
            return Ok(false);
        };
        let expected = progress
            .last_sequence
            .checked_add(1)
            .ok_or(ProtocolError::InvalidSegment("sequence overflow"))?;
        if progress.epoch != next.source_epoch
            || expected != first_missing
            || last_missing.checked_add(1) != Some(next.sequence)
        {
            return Ok(false);
        }
        progress.last_sequence = last_missing;
        Ok(true)
    }

    pub(crate) fn is_tombstoned(&self, cursor: &Cursor, record: &SyncRecord) -> bool {
        self.tombstones.contains(&TombstoneKey::new(cursor, record))
    }

    pub(crate) fn is_quarantined(&self, cursor: &Cursor) -> bool {
        self.quarantined_streams.contains_key(&cursor.stream_key())
    }

    pub(crate) fn continuous_watermark(
        &self,
        cursor: &Cursor,
    ) -> Result<Option<Cursor>, ProtocolError> {
        self.streams
            .get(&cursor.stream_key())
            .filter(|progress| progress.epoch == cursor.source_epoch)
            .map(|progress| progress.watermark(cursor))
            .transpose()
    }

    pub(crate) fn forwardable_unknown_segments(&self) -> &[SignedSegment] {
        &self.forwardable_unknown_segments
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
struct StreamKey {
    source_node_id: String,
    stream: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
struct TombstoneKey {
    stream: StreamKey,
    source_epoch: u64,
    schema_id: String,
    schema_version: u32,
    subject_node_id: String,
    observer_node_id: String,
    record_key: Vec<u8>,
}

impl TombstoneKey {
    fn new(cursor: &Cursor, record: &SyncRecord) -> Self {
        Self {
            stream: cursor.stream_key(),
            source_epoch: cursor.source_epoch,
            schema_id: record.schema_id.clone(),
            schema_version: record.schema_version,
            subject_node_id: record.subject_node_id.clone(),
            observer_node_id: record.observer_node_id.clone(),
            record_key: record.record_key.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StreamProgress {
    epoch: u64,
    last_sequence: u64,
    last_segment_hash: [u8; 32],
    recent_segments: VecDeque<SegmentHashRange>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct SegmentHashRange {
    first_sequence: u64,
    last_sequence: u64,
    hash: [u8; 32],
}

impl StreamProgress {
    fn watermark(&self, cursor: &Cursor) -> Result<Cursor, ProtocolError> {
        cursor.with_sequence(self.last_sequence)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProtocolError {
    InvalidSegment(&'static str),
    SegmentRecordLimit { actual: usize },
    SegmentCanonicalLimit { actual: usize },
    SegmentDurationLimit,
    ResponseCanonicalLimit { actual: usize },
    WireLimit { actual: usize },
    CompressionFailed,
    CompressionExpansionLimit,
    InvalidWire,
    NonCanonicalWire,
    InvalidSignature,
    InvalidSigningKey,
    SourceIdentityMismatch,
    ClusterMismatch,
    UnknownSchemaBacklogFull,
    CursorIdentityMismatch,
    SequenceGap { expected: u64, actual: u64 },
    EpochGap { expected: u64, actual: u64 },
    HashChainMismatch,
    ForkDetected { next_epoch: u64 },
    Quarantined,
    ResurrectionPrevented,
    CheckpointLimit,
    EncodingDecision,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "history sync protocol error: {self:?}")
    }
}

impl std::error::Error for ProtocolError {}

fn hash_records(records: &[SyncRecord]) -> [u8; 32] {
    let mut hash = Sha256::new();
    for record in records {
        let mut encoded = Vec::new();
        record
            .clone()
            .into_proto()
            .encode_length_delimited(&mut encoded)
            .expect("encoding a Vec cannot fail");
        hash.update(encoded);
    }
    hash.finalize().into()
}
