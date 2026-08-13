use std::collections::{BTreeMap, BTreeSet};

use super::ReplicaError;

pub(crate) const ANTI_ENTROPY_INTERVAL_SECONDS: u64 = 5 * 60;
pub(crate) const DEEP_VERIFICATION_INTERVAL_SECONDS: u64 = 24 * 60 * 60;
const MAX_REPAIR_RANGES: usize = 64;
const MAX_RECORDS_PER_REPAIR_RANGE: u64 = 1_000;
const MAX_UNKNOWN_SCHEMA_BYTES: usize = 64 * 1024;
const MAX_UNKNOWN_SCHEMA_SEGMENTS: usize = 64;
const MAX_PARTITION_SUMMARIES: usize = 1_024;
const MAX_TOMBSTONES: usize = 8_192;
const MAX_TRACKED_STREAMS: usize = 4_096;
const MAX_FORK_HASHES_PER_STREAM: usize = 1_000;
const MAX_PERMANENT_GAPS: usize = 64;
const MAX_RECORD_BYTES: usize = 256 * 1024;
const MAX_TOMBSTONE_READY_REPOSITORIES: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplicaWork {
    None,
    AntiEntropy,
    DeepVerification,
}

impl ReplicaWork {
    pub(crate) fn is_anti_entropy(self) -> bool {
        matches!(self, Self::AntiEntropy | Self::DeepVerification)
    }

    pub(crate) fn is_deep_verification(self) -> bool {
        matches!(self, Self::DeepVerification)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AntiEntropySchedule {
    anti_entropy_interval_seconds: u64,
    deep_verification_interval_seconds: u64,
}

impl Default for AntiEntropySchedule {
    fn default() -> Self {
        Self {
            anti_entropy_interval_seconds: ANTI_ENTROPY_INTERVAL_SECONDS,
            deep_verification_interval_seconds: DEEP_VERIFICATION_INTERVAL_SECONDS,
        }
    }
}

impl AntiEntropySchedule {
    pub(crate) fn due(
        self,
        now_unix_seconds: u64,
        last_deep_verification: Option<u64>,
        last_anti_entropy: Option<u64>,
    ) -> ReplicaWork {
        if last_deep_verification.is_none_or(|last| {
            now_unix_seconds.saturating_sub(last) >= self.deep_verification_interval_seconds
        }) {
            return ReplicaWork::DeepVerification;
        }
        if last_anti_entropy.is_none_or(|last| {
            now_unix_seconds.saturating_sub(last) >= self.anti_entropy_interval_seconds
        }) {
            return ReplicaWork::AntiEntropy;
        }
        ReplicaWork::None
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ReplicaPartition {
    source_node_id: String,
    source_epoch: u64,
    stream: String,
    partition: u32,
}

impl ReplicaPartition {
    pub(crate) fn new(
        source_node_id: impl Into<String>,
        source_epoch: u64,
        stream: impl Into<String>,
        partition: u32,
    ) -> Result<Self, ReplicaError> {
        let source_node_id = source_node_id.into();
        let stream = stream.into();
        validate_identifier(&source_node_id)?;
        validate_identifier(&stream)?;
        Ok(Self {
            source_node_id,
            source_epoch,
            stream,
            partition,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PartitionSummary {
    partition: ReplicaPartition,
    first_sequence: u64,
    last_sequence: u64,
    hash: [u8; 32],
    record_count: u64,
}

impl PartitionSummary {
    pub(crate) fn new(
        partition: ReplicaPartition,
        first_sequence: u64,
        last_sequence: u64,
        hash: [u8; 32],
        record_count: u64,
    ) -> Result<Self, ReplicaError> {
        if first_sequence > last_sequence
            || last_sequence
                .checked_sub(first_sequence)
                .and_then(|span| span.checked_add(1))
                != Some(record_count)
        {
            return Err(ReplicaError::InvalidRange);
        }
        Ok(Self {
            partition,
            first_sequence,
            last_sequence,
            hash,
            record_count,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepairRange {
    partition: ReplicaPartition,
    first_sequence: u64,
    last_sequence: u64,
}

impl RepairRange {
    pub(crate) fn partition(&self) -> &ReplicaPartition {
        &self.partition
    }

    pub(crate) fn first_sequence(&self) -> u64 {
        self.first_sequence
    }

    pub(crate) fn last_sequence(&self) -> u64 {
        self.last_sequence
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RepairPlan {
    ranges: Vec<RepairRange>,
}

impl RepairPlan {
    pub(crate) fn between(
        local: impl IntoIterator<Item = PartitionSummary>,
        remote: impl IntoIterator<Item = PartitionSummary>,
    ) -> Result<Self, ReplicaError> {
        let local = summaries_by_partition(local)?;
        let remote = summaries_by_partition(remote)?;
        let partitions: BTreeSet<_> = local.keys().chain(remote.keys()).cloned().collect();
        let mut ranges = Vec::new();
        for partition in partitions {
            let left = local.get(&partition);
            let right = remote.get(&partition);
            if left.is_some_and(|summary| right == Some(summary)) {
                continue;
            }
            let (first_sequence, last_sequence) = match (left, right) {
                (Some(left), Some(right)) => (
                    left.first_sequence.min(right.first_sequence),
                    left.last_sequence.max(right.last_sequence),
                ),
                (Some(summary), None) | (None, Some(summary)) => {
                    (summary.first_sequence, summary.last_sequence)
                }
                (None, None) => continue,
            };
            append_bounded_ranges(&mut ranges, partition, first_sequence, last_sequence)?;
        }
        Ok(Self { ranges })
    }

    pub(crate) fn ranges(&self) -> &[RepairRange] {
        &self.ranges
    }

    pub(crate) fn is_converged(&self) -> bool {
        self.ranges.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ReplicaCursor {
    source_node_id: String,
    source_epoch: u64,
    stream: String,
    sequence: u64,
}

impl ReplicaCursor {
    pub(crate) fn new(
        source_node_id: impl Into<String>,
        source_epoch: u64,
        stream: impl Into<String>,
        sequence: u64,
    ) -> Result<Self, ReplicaError> {
        let source_node_id = source_node_id.into();
        let stream = stream.into();
        validate_identifier(&source_node_id)?;
        validate_identifier(&stream)?;
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReplicaConvergence {
    repair: RepairPlan,
    permanent_gaps: Vec<ReplicaCursor>,
}

impl ReplicaConvergence {
    pub(crate) fn from_summaries(
        repair: RepairPlan,
        reported_gaps: impl IntoIterator<Item = ReplicaCursor>,
    ) -> Result<Self, ReplicaError> {
        let mut permanent_gaps = Vec::with_capacity(MAX_PERMANENT_GAPS);
        for gap in reported_gaps {
            if !permanent_gaps.contains(&gap) {
                if permanent_gaps.len() == MAX_PERMANENT_GAPS {
                    return Err(ReplicaError::RepairLimitExceeded);
                }
                permanent_gaps.push(gap);
            }
        }
        permanent_gaps.sort();
        Ok(Self {
            repair,
            permanent_gaps,
        })
    }

    pub(crate) fn is_converged(&self) -> bool {
        self.repair.is_converged() && self.permanent_gaps.is_empty()
    }

    pub(crate) fn has_permanent_gaps(&self) -> bool {
        !self.permanent_gaps.is_empty()
    }

    pub(crate) fn permanent_gaps(&self) -> &[ReplicaCursor] {
        &self.permanent_gaps
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ReplicaRecordKey {
    schema_id: String,
    schema_version: u32,
    record_key: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReplicaRecord {
    key: ReplicaRecordKey,
    payload: Vec<u8>,
}

impl ReplicaRecord {
    pub(crate) fn new(
        schema_id: impl Into<String>,
        schema_version: u32,
        record_key: Vec<u8>,
        payload: Vec<u8>,
    ) -> Result<Self, ReplicaError> {
        let schema_id = schema_id.into();
        validate_identifier(&schema_id)?;
        if record_key.len().saturating_add(payload.len()) > MAX_RECORD_BYTES {
            return Err(ReplicaError::RecordTooLarge);
        }
        Ok(Self {
            key: ReplicaRecordKey {
                schema_id,
                schema_version,
                record_key,
            },
            payload,
        })
    }

    pub(crate) fn key(&self) -> ReplicaRecordKey {
        self.key.clone()
    }
}

#[derive(Debug, Clone)]
struct TombstoneState {
    expires_at: u64,
    ready_repositories: BTreeSet<String>,
    acknowledgements: BTreeSet<String>,
}

#[derive(Debug, Default)]
pub(crate) struct TombstoneLedger {
    horizon_seconds: u64,
    entries: BTreeMap<ReplicaRecordKey, TombstoneState>,
}

impl TombstoneLedger {
    pub(crate) fn new(horizon_seconds: u64) -> Self {
        Self {
            horizon_seconds,
            entries: BTreeMap::new(),
        }
    }

    pub(crate) fn tombstone(
        &mut self,
        key: ReplicaRecordKey,
        now_unix_seconds: u64,
        ready_repositories: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<(), ReplicaError> {
        let mut repositories = BTreeSet::new();
        for repository in ready_repositories {
            let repository = repository.as_ref();
            validate_identifier(repository)?;
            if !repositories.contains(repository)
                && repositories.len() == MAX_TOMBSTONE_READY_REPOSITORIES
            {
                return Err(ReplicaError::RepositoryBacklogFull);
            }
            repositories.insert(repository.to_owned());
        }
        if repositories.is_empty() {
            return Err(ReplicaError::EmptyRepositories);
        }
        if !self.entries.contains_key(&key) && self.entries.len() == MAX_TOMBSTONES {
            return Err(ReplicaError::TombstoneBacklogFull);
        }
        self.entries.insert(
            key,
            TombstoneState {
                expires_at: now_unix_seconds.saturating_add(self.horizon_seconds),
                ready_repositories: repositories,
                acknowledgements: BTreeSet::new(),
            },
        );
        Ok(())
    }

    pub(crate) fn acknowledge(
        &mut self,
        key: ReplicaRecordKey,
        repository_id: &str,
    ) -> Result<(), ReplicaError> {
        validate_identifier(repository_id)?;
        let state = self
            .entries
            .get_mut(&key)
            .ok_or(ReplicaError::TombstoneMissing)?;
        if state.ready_repositories.contains(repository_id) {
            state.acknowledgements.insert(repository_id.to_owned());
        }
        Ok(())
    }

    pub(crate) fn allows(&self, key: &ReplicaRecordKey) -> bool {
        !self.entries.contains_key(key)
    }

    pub(crate) fn expire(&mut self, now_unix_seconds: u64) -> usize {
        let before = self.entries.len();
        self.entries.retain(|_, state| {
            now_unix_seconds < state.expires_at
                || !state.ready_repositories.is_subset(&state.acknowledgements)
        });
        before - self.entries.len()
    }
}

#[derive(Debug, Default)]
pub(crate) struct StreamForkGuard {
    streams: BTreeMap<(String, String), StreamForkState>,
}

#[derive(Debug)]
struct StreamForkState {
    epoch: u64,
    hashes: BTreeMap<u64, [u8; 32]>,
    quarantined: bool,
}

impl StreamForkGuard {
    pub(crate) fn observe(
        &mut self,
        cursor: &ReplicaCursor,
        payload_hash: [u8; 32],
    ) -> Result<(), ReplicaError> {
        let key = (cursor.source_node_id.clone(), cursor.stream.clone());
        if !self.streams.contains_key(&key) && self.streams.len() == MAX_TRACKED_STREAMS {
            return Err(ReplicaError::StreamBacklogFull);
        }
        let state = self.streams.entry(key).or_insert_with(|| StreamForkState {
            epoch: cursor.source_epoch,
            hashes: BTreeMap::new(),
            quarantined: false,
        });
        if state.quarantined {
            return Err(ReplicaError::ForkQuarantined {
                next_epoch: state.epoch.saturating_add(1),
            });
        }
        if state.epoch != cursor.source_epoch {
            return Err(ReplicaError::ForkQuarantined {
                next_epoch: state.epoch.saturating_add(1),
            });
        }
        if let Some(existing) = state.hashes.get(&cursor.sequence)
            && existing != &payload_hash
        {
            state.quarantined = true;
            return Err(ReplicaError::ForkQuarantined {
                next_epoch: state.epoch.saturating_add(1),
            });
        }
        state.hashes.insert(cursor.sequence, payload_hash);
        if state.hashes.len() > MAX_FORK_HASHES_PER_STREAM {
            state.hashes.pop_first();
        }
        Ok(())
    }

    pub(crate) fn start_new_epoch(
        &mut self,
        source_node_id: &str,
        stream: &str,
        epoch: u64,
    ) -> Result<(), ReplicaError> {
        validate_identifier(source_node_id)?;
        validate_identifier(stream)?;
        let key = (source_node_id.to_owned(), stream.to_owned());
        if let Some(state) = self.streams.get(&key)
            && epoch <= state.epoch
        {
            return Err(ReplicaError::EpochNotAdvanced {
                minimum_epoch: state.epoch.saturating_add(1),
            });
        }
        if !self.streams.contains_key(&key) && self.streams.len() == MAX_TRACKED_STREAMS {
            return Err(ReplicaError::StreamBacklogFull);
        }
        self.streams.insert(
            key,
            StreamForkState {
                epoch,
                hashes: BTreeMap::new(),
                quarantined: false,
            },
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ReplicaFreshness {
    last_verified_unix_seconds: u64,
    tombstone_horizon_seconds: u64,
}

impl ReplicaFreshness {
    pub(crate) fn new(last_verified_unix_seconds: u64, tombstone_horizon_seconds: u64) -> Self {
        Self {
            last_verified_unix_seconds,
            tombstone_horizon_seconds,
        }
    }

    pub(crate) fn requires_rebuild(self, now_unix_seconds: u64) -> bool {
        now_unix_seconds
            > self
                .last_verified_unix_seconds
                .saturating_add(self.tombstone_horizon_seconds)
    }
}

#[derive(Debug, Default)]
pub(crate) struct UnknownSchemaBuffer {
    byte_limit: usize,
    bytes_used: usize,
    entries: BTreeMap<(String, u32), Vec<Vec<u8>>>,
}

impl UnknownSchemaBuffer {
    pub(crate) fn new(byte_limit: usize) -> Self {
        Self {
            byte_limit: byte_limit.min(MAX_UNKNOWN_SCHEMA_BYTES),
            ..Self::default()
        }
    }

    pub(crate) fn store(
        &mut self,
        schema_id: &str,
        schema_version: u32,
        raw_signed_segment: Vec<u8>,
    ) -> Result<(), ReplicaError> {
        validate_identifier(schema_id)?;
        let next_size = self.bytes_used.saturating_add(raw_signed_segment.len());
        if self.entries.values().map(Vec::len).sum::<usize>() == MAX_UNKNOWN_SCHEMA_SEGMENTS
            || next_size > self.byte_limit
        {
            return Err(ReplicaError::UnknownSchemaBacklogFull);
        }
        self.entries
            .entry((schema_id.to_owned(), schema_version))
            .or_default()
            .push(raw_signed_segment);
        self.bytes_used = next_size;
        Ok(())
    }

    pub(crate) fn is_forwardable(&self, schema_id: &str, schema_version: u32) -> bool {
        self.entries
            .contains_key(&(schema_id.to_owned(), schema_version))
    }

    pub(crate) fn is_queryable(&self, _schema_id: &str, _schema_version: u32) -> bool {
        false
    }
}

fn summaries_by_partition(
    summaries: impl IntoIterator<Item = PartitionSummary>,
) -> Result<BTreeMap<ReplicaPartition, PartitionSummary>, ReplicaError> {
    let mut result = BTreeMap::new();
    for summary in summaries {
        if result.len() == MAX_PARTITION_SUMMARIES {
            return Err(ReplicaError::RepairLimitExceeded);
        }
        if result.insert(summary.partition.clone(), summary).is_some() {
            return Err(ReplicaError::InvalidRange);
        }
    }
    Ok(result)
}

fn append_bounded_ranges(
    ranges: &mut Vec<RepairRange>,
    partition: ReplicaPartition,
    first_sequence: u64,
    last_sequence: u64,
) -> Result<(), ReplicaError> {
    let mut first = first_sequence;
    while first <= last_sequence {
        if ranges.len() == MAX_REPAIR_RANGES {
            return Err(ReplicaError::RepairLimitExceeded);
        }
        let last = first
            .saturating_add(MAX_RECORDS_PER_REPAIR_RANGE.saturating_sub(1))
            .min(last_sequence);
        ranges.push(RepairRange {
            partition: partition.clone(),
            first_sequence: first,
            last_sequence: last,
        });
        if last == u64::MAX {
            break;
        }
        first = last + 1;
    }
    Ok(())
}

fn validate_identifier(value: &str) -> Result<(), ReplicaError> {
    if value.trim().is_empty() || value != value.trim() || value.chars().any(char::is_control) {
        return Err(ReplicaError::InvalidIdentifier);
    }
    Ok(())
}
