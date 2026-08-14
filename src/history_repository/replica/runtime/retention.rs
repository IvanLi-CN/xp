use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{StoredGap, StoredRecord};

pub(super) fn prune_records(
    records: &mut Vec<StoredRecord>,
    gaps: &[StoredGap],
    now_unix_seconds: u64,
    cluster_id: Option<&str>,
) {
    let policy = super::super::RepositoryRetentionPolicy::default();
    let mut retained = BTreeMap::<RetentionBucket, RetainedRecord>::new();
    for record in records.drain(..) {
        if record.tombstone {
            retained.insert(
                RetentionBucket::tombstone(&record),
                RetainedRecord::Raw(record),
            );
            continue;
        }
        let age = now_unix_seconds.saturating_sub(record.observed_at_unix_seconds);
        let Some(resolution) = policy.resolution_for_age(age) else {
            continue;
        };
        let preserves_minute_detail = policy.keeps_minute_detail(age);
        let bucket =
            RetentionBucket::for_record(&record, resolution, preserves_minute_detail, cluster_id);
        match retained.entry(bucket) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                if preserves_minute_detail {
                    entry.insert(RetainedRecord::Raw(record));
                } else {
                    entry.insert(RetainedRecord::Aggregate(RetentionAggregate::from_record(
                        record, resolution, cluster_id,
                    )));
                }
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => match entry.get_mut() {
                RetainedRecord::Raw(existing) if record.sequence >= existing.sequence => {
                    *existing = record;
                }
                RetainedRecord::Raw(_) => {}
                RetainedRecord::Aggregate(aggregate) => aggregate.add(record),
            },
        }
    }
    *records = retained
        .into_values()
        .map(|record| record.into_stored_record(gaps))
        .collect();
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RetentionBucket {
    source_node_id: String,
    source_epoch: u64,
    stream: String,
    subject_node_id: String,
    observer_node_id: String,
    schema_id: String,
    schema_version: u32,
    record_key: Option<Vec<u8>>,
    bucket_start: u64,
}

impl RetentionBucket {
    fn for_record(
        record: &StoredRecord,
        resolution: super::super::RetentionResolution,
        preserves_minute_detail: bool,
        cluster_id: Option<&str>,
    ) -> Self {
        let seconds = match resolution {
            super::super::RetentionResolution::Minute => 60,
            super::super::RetentionResolution::FiveMinutes => 5 * 60,
            super::super::RetentionResolution::Hour => 60 * 60,
        };
        Self::with_bucket(
            record,
            record.observed_at_unix_seconds / seconds * seconds,
            preserves_minute_detail,
            cluster_id,
        )
    }

    fn tombstone(record: &StoredRecord) -> Self {
        Self::with_bucket(record, record.observed_at_unix_seconds, true, None)
    }

    fn with_bucket(
        record: &StoredRecord,
        bucket_start: u64,
        preserves_minute_detail: bool,
        cluster_id: Option<&str>,
    ) -> Self {
        Self {
            source_node_id: record.source_node_id.clone(),
            source_epoch: record.source_epoch,
            stream: record.stream.clone(),
            subject_node_id: record.subject_node_id.clone(),
            observer_node_id: record.observer_node_id.clone(),
            schema_id: record.schema_id.clone(),
            schema_version: record.schema_version,
            record_key: retention_identifier(record, preserves_minute_detail, cluster_id),
            bucket_start,
        }
    }
}

fn retention_identifier(
    record: &StoredRecord,
    preserves_minute_detail: bool,
    cluster_id: Option<&str>,
) -> Option<Vec<u8>> {
    if preserves_minute_detail {
        return Some(record.record_key.clone());
    }
    (record.schema_id == "ip_usage.v1").then(|| {
        aggregate_contribution(record)
            .anonymized_identifier
            .unwrap_or_else(|| {
                anonymized_identifier(cluster_id, &record.subject_node_id, &record.record_key)
            })
            .into_bytes()
    })
}

enum RetainedRecord {
    Raw(StoredRecord),
    Aggregate(RetentionAggregate),
}

impl RetainedRecord {
    fn into_stored_record(self, gaps: &[StoredGap]) -> StoredRecord {
        match self {
            Self::Raw(record) => record,
            Self::Aggregate(aggregate) => aggregate.into_stored_record(gaps),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct RetentionAggregatePayload {
    algorithm: String,
    resolution: String,
    record_count: u64,
    first_sequence: u64,
    last_sequence: u64,
    payload_sha256: String,
    complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    anonymized_identifier: Option<String>,
}

struct RetentionAggregate {
    first: StoredRecord,
    resolution: super::super::RetentionResolution,
    cluster_id: Option<String>,
    record_count: u64,
    first_sequence: u64,
    last_sequence: u64,
    payload_hash: [u8; 32],
    complete: bool,
    anonymized_identifier: Option<String>,
}

impl RetentionAggregate {
    fn from_record(
        record: StoredRecord,
        resolution: super::super::RetentionResolution,
        cluster_id: Option<&str>,
    ) -> Self {
        let contribution = aggregate_contribution(&record);
        Self {
            first_sequence: contribution.first_sequence,
            last_sequence: contribution.last_sequence,
            first: record,
            resolution,
            cluster_id: cluster_id.map(ToOwned::to_owned),
            record_count: contribution.record_count,
            payload_hash: contribution.payload_hash,
            complete: contribution.complete,
            anonymized_identifier: contribution.anonymized_identifier,
        }
    }

    fn add(&mut self, record: StoredRecord) {
        let contribution = aggregate_contribution(&record);
        self.first_sequence = self.first_sequence.min(contribution.first_sequence);
        self.last_sequence = self.last_sequence.max(contribution.last_sequence);
        self.record_count = self.record_count.saturating_add(contribution.record_count);
        self.payload_hash = combine_aggregate_hashes(self.payload_hash, contribution.payload_hash);
        self.complete &= contribution.complete;
        if contribution.last_sequence >= self.first.sequence {
            self.first = record;
        }
        if self.anonymized_identifier.is_none() {
            self.anonymized_identifier = contribution.anonymized_identifier;
        }
    }

    fn into_stored_record(self, gaps: &[StoredGap]) -> StoredRecord {
        let mut record = self.first;
        let resolution = match self.resolution {
            super::super::RetentionResolution::Minute => "minute",
            super::super::RetentionResolution::FiveMinutes => "five_minutes",
            super::super::RetentionResolution::Hour => "hour",
        };
        let incomplete = gaps.iter().any(|gap| {
            gap.source_node_id == record.source_node_id
                && gap.source_epoch == record.source_epoch
                && gap.stream == record.stream
                && gap.first_sequence <= self.last_sequence
                && gap.last_sequence >= self.first_sequence
        });
        let anonymized_identifier = self.anonymized_identifier.or_else(|| {
            (record.schema_id == "ip_usage.v1").then(|| {
                anonymized_identifier(
                    self.cluster_id.as_deref(),
                    &record.subject_node_id,
                    &record.record_key,
                )
            })
        });
        let payload = RetentionAggregatePayload {
            algorithm: "sha256".to_owned(),
            resolution: resolution.to_owned(),
            record_count: self.record_count,
            first_sequence: self.first_sequence,
            last_sequence: self.last_sequence,
            payload_sha256: hex::encode(self.payload_hash),
            complete: self.complete && !incomplete,
            anonymized_identifier,
        };
        record.sequence = self.last_sequence;
        record.record_key = aggregate_record_key(&record, resolution);
        record.payload = serde_json::to_vec(&payload).expect("retention aggregate is serializable");
        record
    }
}

struct AggregateContribution {
    record_count: u64,
    first_sequence: u64,
    last_sequence: u64,
    payload_hash: [u8; 32],
    complete: bool,
    anonymized_identifier: Option<String>,
}

fn aggregate_contribution(record: &StoredRecord) -> AggregateContribution {
    let parsed = serde_json::from_slice::<RetentionAggregatePayload>(&record.payload)
        .ok()
        .and_then(|payload| {
            let hash = hex::decode(&payload.payload_sha256).ok()?;
            let payload_hash: [u8; 32] = hash.try_into().ok()?;
            (payload.algorithm == "sha256"
                && payload.record_count > 0
                && payload.first_sequence <= payload.last_sequence)
                .then_some(AggregateContribution {
                    record_count: payload.record_count,
                    first_sequence: payload.first_sequence,
                    last_sequence: payload.last_sequence,
                    payload_hash,
                    complete: payload.complete,
                    anonymized_identifier: payload.anonymized_identifier,
                })
        });
    parsed.unwrap_or_else(|| AggregateContribution {
        record_count: 1,
        first_sequence: record.sequence,
        last_sequence: record.sequence,
        payload_hash: record_payload_hash(record),
        complete: true,
        anonymized_identifier: None,
    })
}

fn record_payload_hash(record: &StoredRecord) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(record.payload.as_slice());
    hasher.update(record.record_key.as_slice());
    hasher.finalize().into()
}

fn combine_aggregate_hashes(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(left);
    hasher.update(right);
    hasher.finalize().into()
}

fn aggregate_record_key(record: &StoredRecord, resolution: &str) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(b"xp-history-repository-aggregate-v1\0");
    hasher.update(record.source_node_id.as_bytes());
    hasher.update(record.source_epoch.to_be_bytes());
    hasher.update(record.stream.as_bytes());
    hasher.update(record.schema_id.as_bytes());
    hasher.update(resolution.as_bytes());
    hasher.update(record.observed_at_unix_seconds.to_be_bytes());
    hasher.finalize().to_vec()
}

fn anonymized_identifier(
    cluster_id: Option<&str>,
    subject_node_id: &str,
    raw_identifier: &[u8],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"xp-history-repository-ip-anonymization-v1\0");
    hasher.update(cluster_id.unwrap_or_default().as_bytes());
    hasher.update(subject_node_id.as_bytes());
    hasher.update(raw_identifier);
    hex::encode(hasher.finalize())
}
