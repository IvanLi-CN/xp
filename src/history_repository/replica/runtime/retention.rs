use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{
    resource_monitoring::{RESOURCE_HISTORY_SCHEMA, ResourceHistoryPayload, ResourceRollup},
    uptime_monitor::{UPTIME_HISTORY_SCHEMA, UptimeHistoryPayload},
};

use super::{StoredGap, StoredRecord};

const RESOURCE_MINUTE_RETENTION_SECONDS: u64 = 14 * 24 * 60 * 60;
const RESOURCE_15_MINUTE_RETENTION_SECONDS: u64 = 90 * 24 * 60 * 60;
const RESOURCE_HOUR_RETENTION_SECONDS: u64 = 365 * 24 * 60 * 60;

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
        let resolution = if record.schema_id == RESOURCE_HISTORY_SCHEMA {
            resource_resolution_for_age(age)
        } else {
            policy.resolution_for_age(age)
        };
        let Some(resolution) = resolution else {
            continue;
        };
        let preserves_minute_detail = if record.schema_id == RESOURCE_HISTORY_SCHEMA {
            age <= RESOURCE_MINUTE_RETENTION_SECONDS
        } else {
            policy.keeps_minute_detail(age)
        };
        let bucket =
            RetentionBucket::for_record(&record, resolution, preserves_minute_detail, cluster_id);
        match retained.entry(bucket) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                if preserves_minute_detail {
                    entry.insert(RetainedRecord::Raw(record));
                } else {
                    entry.insert(RetainedRecord::Aggregate(Box::new(
                        RetentionAggregate::from_record(record, resolution, cluster_id),
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
        Self::with_bucket(
            record,
            bucket_time_range_for_record(record, resolution).0,
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

pub(super) fn record_time_range(record: &StoredRecord) -> (u64, u64) {
    if record.schema_id == RESOURCE_HISTORY_SCHEMA
        && let Some(payload) = resource_payload(record)
    {
        return match payload {
            ResourceHistoryPayload::Rollup { rollup, resolution } => {
                let start = rollup.bucket_start_unix_seconds.max(0) as u64;
                let seconds: u64 = match resolution.as_str() {
                    "15m" => 15 * 60,
                    "1h" => 60 * 60,
                    _ => 60,
                };
                (start, start.saturating_add(seconds.saturating_sub(1)))
            }
            ResourceHistoryPayload::CaptureGap { gap, .. } => (
                gap.from_bucket_start_unix_seconds.max(0) as u64,
                gap.to_bucket_start_unix_seconds.max(0) as u64,
            ),
        };
    }
    aggregate_payload(record)
        .and_then(|payload| {
            payload
                .bucket_start_unix_seconds
                .zip(payload.bucket_end_unix_seconds)
                .filter(|(start, end)| start <= end)
        })
        .unwrap_or((
            record.observed_at_unix_seconds,
            record.observed_at_unix_seconds,
        ))
}

/// An aggregate bucket may only be compacted once its end is before the page boundary. This
/// keeps a keyset page from producing two independent aggregates for one five-minute/hourly
/// bucket when its final source samples are on the next page.
pub(super) fn compaction_bucket_end(record: &StoredRecord, now_unix_seconds: u64) -> Option<u64> {
    if record.tombstone {
        return None;
    }
    let policy = super::super::RepositoryRetentionPolicy::default();
    let resolution = if record.schema_id == RESOURCE_HISTORY_SCHEMA {
        resource_resolution_for_age(
            now_unix_seconds.saturating_sub(record.observed_at_unix_seconds),
        )
    } else {
        policy.resolution_for_age(now_unix_seconds.saturating_sub(record.observed_at_unix_seconds))
    }?;
    Some(bucket_time_range_for_record(record, resolution).1)
}

/// Returns whether a record's time bucket can still receive rows after a keyset page boundary.
/// Bucket identity also includes subject/schema dimensions, but the time boundary is shared by
/// all of those identities; retaining every matching aggregate prevents interleaved subjects from
/// being split across pages.
pub(super) fn compaction_bucket_reaches(
    record: &StoredRecord,
    page_boundary_unix_seconds: u64,
    now_unix_seconds: u64,
) -> bool {
    compaction_bucket_end(record, now_unix_seconds)
        .is_some_and(|bucket_end| bucket_end >= page_boundary_unix_seconds)
}

pub(super) fn incomplete_aggregate_gap<'a>(
    records: impl IntoIterator<Item = &'a StoredRecord>,
) -> Option<(u64, u64)> {
    records
        .into_iter()
        .filter(|record| aggregate_payload(record).is_some_and(|payload| !payload.complete))
        .map(record_time_range)
        .fold(None, |range, (start, end)| match range {
            Some((current_start, current_end)) => {
                Some((current_start.min(start), current_end.max(end)))
            }
            None => Some((start, end)),
        })
}

pub(super) fn aggregate_metadata(record: &StoredRecord) -> Option<(bool, u64, u64)> {
    let payload = aggregate_payload(record)?;
    let (start, end) = payload
        .bucket_start_unix_seconds
        .zip(payload.bucket_end_unix_seconds)
        .filter(|(start, end)| start <= end)?;
    Some((payload.complete, start, end))
}

fn retention_identifier(
    record: &StoredRecord,
    preserves_minute_detail: bool,
    cluster_id: Option<&str>,
) -> Option<Vec<u8>> {
    if preserves_minute_detail {
        return Some(record.record_key.clone());
    }
    if record.schema_id == UPTIME_HISTORY_SCHEMA {
        return serde_json::from_slice::<UptimeHistoryPayload>(&record.payload)
            .ok()
            .map(|payload| {
                format!(
                    "{}:{}:{}:{}",
                    payload.monitor_id, payload.revision, payload.observer_node_id, payload.ad_hoc
                )
                .into_bytes()
            });
    }
    if record.schema_id == RESOURCE_HISTORY_SCHEMA {
        return resource_payload(record).map(|payload| {
            match payload {
                ResourceHistoryPayload::Rollup { .. } => "resource:rollup",
                ResourceHistoryPayload::CaptureGap { .. } => "resource:gap",
            }
            .as_bytes()
            .to_vec()
        });
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
    Aggregate(Box<RetentionAggregate>),
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bucket_start_unix_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bucket_end_unix_seconds: Option<u64>,
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
    bucket_start_unix_seconds: u64,
    bucket_end_unix_seconds: u64,
    cluster_id: Option<String>,
    record_count: u64,
    first_sequence: u64,
    last_sequence: u64,
    payload_hash: [u8; 32],
    complete: bool,
    anonymized_identifier: Option<String>,
    uptime_payload: Option<UptimeHistoryPayload>,
    resource_payload: Option<ResourceHistoryPayload>,
}

impl RetentionAggregate {
    fn from_record(
        record: StoredRecord,
        resolution: super::super::RetentionResolution,
        cluster_id: Option<&str>,
    ) -> Self {
        let contribution = aggregate_contribution(&record);
        let uptime_payload = uptime_payload(&record);
        let resource_payload = resource_payload(&record);
        let (bucket_start_unix_seconds, bucket_end_unix_seconds) =
            bucket_time_range_for_record(&record, resolution);
        Self {
            first_sequence: contribution.first_sequence,
            last_sequence: contribution.last_sequence,
            first: record,
            resolution,
            bucket_start_unix_seconds,
            bucket_end_unix_seconds,
            cluster_id: cluster_id.map(ToOwned::to_owned),
            record_count: contribution.record_count,
            payload_hash: contribution.payload_hash,
            complete: contribution.complete,
            anonymized_identifier: contribution.anonymized_identifier,
            uptime_payload,
            resource_payload,
        }
    }

    fn add(&mut self, record: StoredRecord) {
        let contribution = aggregate_contribution(&record);
        let incoming_uptime_payload = uptime_payload(&record);
        let incoming_resource_payload = resource_payload(&record);
        let (bucket_start_unix_seconds, bucket_end_unix_seconds) =
            bucket_time_range_for_record(&record, self.resolution);
        self.bucket_start_unix_seconds = self
            .bucket_start_unix_seconds
            .min(bucket_start_unix_seconds);
        self.bucket_end_unix_seconds = self.bucket_end_unix_seconds.max(bucket_end_unix_seconds);
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
        self.uptime_payload = match (self.uptime_payload.take(), incoming_uptime_payload) {
            (Some(mut current), Some(incoming)) => {
                current.merge_rollup(&incoming);
                Some(current)
            }
            _ => None,
        };
        self.resource_payload = match (self.resource_payload.take(), incoming_resource_payload) {
            (Some(mut current), Some(incoming)) => {
                merge_resource_payload(&mut current, incoming);
                Some(current)
            }
            _ => None,
        };
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
                && gap.stream == record.stream
                && ((gap.source_epoch == record.source_epoch
                    && gap.first_sequence <= self.last_sequence
                    && gap.last_sequence >= self.first_sequence)
                    || (gap.permanent
                        && gap.start_unix_seconds <= self.bucket_end_unix_seconds
                        && self.bucket_start_unix_seconds <= gap.end_unix_seconds))
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
        record.record_key = aggregate_record_key(
            &record,
            resolution,
            self.bucket_start_unix_seconds,
            anonymized_identifier.as_deref(),
        );
        if let Some(mut uptime_payload) = self.uptime_payload {
            uptime_payload.resolution = Some(resolution.to_owned());
            uptime_payload.bucket_start_unix_seconds = Some(self.bucket_start_unix_seconds);
            uptime_payload.bucket_end_unix_seconds = Some(self.bucket_end_unix_seconds);
            uptime_payload.record_count = self.record_count;
            uptime_payload.first_sequence = self.first_sequence;
            uptime_payload.last_sequence = self.last_sequence;
            uptime_payload.payload_sha256 = hex::encode(self.payload_hash);
            uptime_payload.complete = self.complete && !incomplete;
            record.sequence = self.last_sequence;
            record.payload = serde_json::to_vec(&uptime_payload)
                .expect("uptime retention aggregate is serializable");
            return record;
        }
        if let Some(mut resource_payload) = self.resource_payload {
            if let ResourceHistoryPayload::Rollup { rollup, .. } = &mut resource_payload {
                rollup.bucket_start_unix_seconds = self.bucket_start_unix_seconds as i64;
            }
            let resource_resolution = match self.resolution {
                super::super::RetentionResolution::Minute => "1m",
                super::super::RetentionResolution::FiveMinutes => "15m",
                super::super::RetentionResolution::Hour => "1h",
            };
            record.sequence = self.last_sequence;
            record.payload = match resource_payload {
                ResourceHistoryPayload::Rollup { rollup, .. } => {
                    serde_json::to_vec(&ResourceHistoryPayload::Rollup {
                        resolution: resource_resolution.to_owned(),
                        rollup,
                    })
                }
                ResourceHistoryPayload::CaptureGap { gap, .. } => {
                    serde_json::to_vec(&ResourceHistoryPayload::CaptureGap {
                        resolution: resource_resolution.to_owned(),
                        gap,
                    })
                }
            }
            .expect("resource retention aggregate is serializable");
            return record;
        }
        let payload = RetentionAggregatePayload {
            algorithm: "sha256".to_owned(),
            resolution: resolution.to_owned(),
            bucket_start_unix_seconds: Some(self.bucket_start_unix_seconds),
            bucket_end_unix_seconds: Some(self.bucket_end_unix_seconds),
            record_count: self.record_count,
            first_sequence: self.first_sequence,
            last_sequence: self.last_sequence,
            payload_sha256: hex::encode(self.payload_hash),
            complete: self.complete && !incomplete,
            anonymized_identifier,
        };
        record.sequence = self.last_sequence;
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
    let parsed = aggregate_payload(record).and_then(|payload| {
        let hash = hex::decode(&payload.payload_sha256).ok()?;
        let payload_hash: [u8; 32] = hash.try_into().ok()?;
        Some(AggregateContribution {
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

fn aggregate_payload(record: &StoredRecord) -> Option<RetentionAggregatePayload> {
    if record.schema_id == UPTIME_HISTORY_SCHEMA {
        let payload = uptime_payload(record)?;
        if payload.is_aggregate()
            && payload.record_count > 0
            && payload.first_sequence <= payload.last_sequence
        {
            return Some(RetentionAggregatePayload {
                algorithm: "sha256".to_owned(),
                resolution: payload.resolution.unwrap_or_default(),
                bucket_start_unix_seconds: payload.bucket_start_unix_seconds,
                bucket_end_unix_seconds: payload.bucket_end_unix_seconds,
                record_count: payload.record_count,
                first_sequence: payload.first_sequence,
                last_sequence: payload.last_sequence,
                payload_sha256: payload.payload_sha256,
                complete: payload.complete,
                anonymized_identifier: None,
            });
        }
    }
    let payload = serde_json::from_slice::<RetentionAggregatePayload>(&record.payload).ok()?;
    (payload.algorithm == "sha256"
        && payload.record_count > 0
        && payload.first_sequence <= payload.last_sequence)
        .then_some(payload)
}

fn uptime_payload(record: &StoredRecord) -> Option<UptimeHistoryPayload> {
    (record.schema_id == UPTIME_HISTORY_SCHEMA)
        .then(|| serde_json::from_slice(&record.payload).ok())
        .flatten()
}

fn resource_payload(record: &StoredRecord) -> Option<ResourceHistoryPayload> {
    (record.schema_id == RESOURCE_HISTORY_SCHEMA)
        .then(|| serde_json::from_slice::<ResourceHistoryPayload>(&record.payload).ok())?
}

fn resource_resolution_for_age(age_seconds: u64) -> Option<super::super::RetentionResolution> {
    if age_seconds <= RESOURCE_MINUTE_RETENTION_SECONDS {
        Some(super::super::RetentionResolution::Minute)
    } else if age_seconds <= RESOURCE_15_MINUTE_RETENTION_SECONDS {
        Some(super::super::RetentionResolution::FiveMinutes)
    } else if age_seconds <= RESOURCE_HOUR_RETENTION_SECONDS {
        Some(super::super::RetentionResolution::Hour)
    } else {
        None
    }
}

fn resource_bucket_seconds(
    record: &StoredRecord,
    resolution: super::super::RetentionResolution,
) -> u64 {
    if record.schema_id == RESOURCE_HISTORY_SCHEMA {
        match resolution {
            super::super::RetentionResolution::Minute => 60,
            super::super::RetentionResolution::FiveMinutes => 15 * 60,
            super::super::RetentionResolution::Hour => 60 * 60,
        }
    } else {
        match resolution {
            super::super::RetentionResolution::Minute => 60,
            super::super::RetentionResolution::FiveMinutes => 5 * 60,
            super::super::RetentionResolution::Hour => 60 * 60,
        }
    }
}

fn bucket_time_range_for_record(
    record: &StoredRecord,
    resolution: super::super::RetentionResolution,
) -> (u64, u64) {
    if record.schema_id == RESOURCE_HISTORY_SCHEMA
        && let Some(payload) = resource_payload(record)
    {
        return match payload {
            ResourceHistoryPayload::Rollup { rollup, .. } => {
                let start = rollup.bucket_start_unix_seconds.max(0) as u64;
                let seconds = resource_bucket_seconds(record, resolution);
                (start, start.saturating_add(seconds.saturating_sub(1)))
            }
            ResourceHistoryPayload::CaptureGap { gap, .. } => (
                gap.from_bucket_start_unix_seconds.max(0) as u64,
                gap.to_bucket_start_unix_seconds.max(0) as u64,
            ),
        };
    }
    bucket_time_range(record.observed_at_unix_seconds, resolution)
}

fn merge_resource_payload(current: &mut ResourceHistoryPayload, incoming: ResourceHistoryPayload) {
    match (current, incoming) {
        (
            ResourceHistoryPayload::Rollup {
                rollup: current, ..
            },
            ResourceHistoryPayload::Rollup {
                rollup: incoming, ..
            },
        ) => merge_resource_rollup(current, &incoming),
        (
            ResourceHistoryPayload::CaptureGap { gap: current, .. },
            ResourceHistoryPayload::CaptureGap { gap: incoming, .. },
        ) => {
            current.from_bucket_start_unix_seconds = current
                .from_bucket_start_unix_seconds
                .min(incoming.from_bucket_start_unix_seconds);
            current.to_bucket_start_unix_seconds = current
                .to_bucket_start_unix_seconds
                .max(incoming.to_bucket_start_unix_seconds);
            if current.reason_code != incoming.reason_code {
                current.reason_code = "multiple_reasons".to_owned();
            }
        }
        _ => {}
    }
}

fn merge_resource_rollup(current: &mut ResourceRollup, incoming: &ResourceRollup) {
    current.bucket_start_unix_seconds = current
        .bucket_start_unix_seconds
        .min(incoming.bucket_start_unix_seconds);
    current.expected_samples = current
        .expected_samples
        .saturating_add(incoming.expected_samples);
    current.captured_samples = current
        .captured_samples
        .saturating_add(incoming.captured_samples);
    current.capability = current.capability.max(incoming.capability);
    for (key, incoming_value) in &incoming.values {
        let value = current
            .values
            .entry(key.clone())
            .or_insert_with(|| incoming_value.clone());
        value.min = match (value.min, incoming_value.min) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (left, right) => left.or(right),
        };
        value.max = match (value.max, incoming_value.max) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (left, right) => left.or(right),
        };
        let left_count = f64::from(
            current
                .captured_samples
                .saturating_sub(incoming.captured_samples),
        );
        let right_count = f64::from(incoming.captured_samples);
        value.mean = match (value.mean, incoming_value.mean) {
            (Some(left), Some(right)) if left_count + right_count > 0.0 => {
                Some((left * left_count + right * right_count) / (left_count + right_count))
            }
            (left, right) => left.or(right),
        };
        value.counter_delta = match (value.counter_delta, incoming_value.counter_delta) {
            (Some(left), Some(right)) => Some(left + right),
            (left, right) => left.or(right),
        };
        value.last = incoming_value.last.or(value.last);
        value.capability = value.capability.max(incoming_value.capability);
    }
}

fn bucket_time_range(
    observed_at_unix_seconds: u64,
    resolution: super::super::RetentionResolution,
) -> (u64, u64) {
    let seconds = match resolution {
        super::super::RetentionResolution::Minute => 60,
        super::super::RetentionResolution::FiveMinutes => 5 * 60,
        super::super::RetentionResolution::Hour => 60 * 60,
    };
    let start = observed_at_unix_seconds / seconds * seconds;
    (start, start.saturating_add(seconds.saturating_sub(1)))
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

fn aggregate_record_key(
    record: &StoredRecord,
    resolution: &str,
    bucket_start: u64,
    anonymized_identifier: Option<&str>,
) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(b"xp-history-repository-aggregate-v1\0");
    hasher.update(record.source_node_id.as_bytes());
    hasher.update(record.source_epoch.to_be_bytes());
    hasher.update(record.stream.as_bytes());
    hasher.update(record.schema_id.as_bytes());
    hasher.update(resolution.as_bytes());
    hasher.update(bucket_start.to_be_bytes());
    if let Some(anonymized_identifier) = anonymized_identifier {
        hasher.update(anonymized_identifier.as_bytes());
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource_monitoring::{
        Capability, ResourceGap, ResourceHistoryPayload, RollupValue,
    };

    #[test]
    fn resource_retention_preserves_numeric_rollup_semantics() {
        const DAY: u64 = 24 * 60 * 60;
        let now = 100 * DAY;
        let observed = now - 20 * DAY;
        let make_record = |sequence: u64, bucket: i64, value: f64| StoredRecord {
            observed_at_unix_seconds: (bucket as u64) + sequence * 60,
            received_at_unix_seconds: now,
            source_node_id: "node-a".to_owned(),
            source_epoch: 1,
            stream: "resource_metrics-v1".to_owned(),
            sequence,
            subject_node_id: "node-a".to_owned(),
            observer_node_id: "node-a".to_owned(),
            schema_id: RESOURCE_HISTORY_SCHEMA.to_owned(),
            schema_version: 1,
            record_key: format!("resource:{bucket}:{sequence}").into_bytes(),
            payload: serde_json::to_vec(&ResourceHistoryPayload::Rollup {
                resolution: "1m".to_owned(),
                rollup: ResourceRollup {
                    node_id: xp_test_fixtures::primary_node_id().to_owned(),
                    bucket_start_unix_seconds: bucket,
                    expected_samples: 4,
                    captured_samples: 4,
                    capability: Capability::Supported,
                    values: [(
                        "domain.cpu_busy_percent".to_owned(),
                        RollupValue {
                            min: Some(value - 1.0),
                            mean: Some(value),
                            max: Some(value + 1.0),
                            last: Some(value),
                            counter_delta: None,
                            capability: Capability::Supported,
                        },
                    )]
                    .into_iter()
                    .collect(),
                },
            })
            .unwrap(),
            tombstone: false,
        };
        let mut records = vec![make_record(1, observed as i64, 10.0)];
        records.push(make_record(2, observed as i64, 30.0));
        prune_records(&mut records, &[], now, None);
        assert_eq!(records.len(), 1);
        let payload: ResourceHistoryPayload = serde_json::from_slice(&records[0].payload).unwrap();
        let ResourceHistoryPayload::Rollup { resolution, rollup } = payload else {
            panic!("expected resource rollup");
        };
        assert_eq!(resolution, "15m");
        assert_eq!(rollup.expected_samples, 8);
        assert_eq!(rollup.captured_samples, 8);
        let value = rollup.values.get("domain.cpu_busy_percent").unwrap();
        assert_eq!(value.min, Some(9.0));
        assert_eq!(value.mean, Some(20.0));
        assert_eq!(value.max, Some(31.0));
        assert_eq!(value.last, Some(30.0));
    }

    #[test]
    fn resource_retention_preserves_capture_gap_payloads() {
        let now = 100 * 24 * 60 * 60;
        let gap = ResourceGap {
            from_bucket_start_unix_seconds: 1_000,
            to_bucket_start_unix_seconds: 1_120,
            reason_code: "journal_full".to_owned(),
        };
        let mut records = vec![StoredRecord {
            observed_at_unix_seconds: now - 8 * 24 * 60 * 60,
            received_at_unix_seconds: now,
            source_node_id: "node-a".to_owned(),
            source_epoch: 1,
            stream: "resource_metrics-v1".to_owned(),
            sequence: 1,
            subject_node_id: "node-a".to_owned(),
            observer_node_id: "node-a".to_owned(),
            schema_id: RESOURCE_HISTORY_SCHEMA.to_owned(),
            schema_version: 1,
            record_key: b"resource-gap".to_vec(),
            payload: serde_json::to_vec(&ResourceHistoryPayload::CaptureGap {
                resolution: "1m".to_owned(),
                gap: gap.clone(),
            })
            .unwrap(),
            tombstone: false,
        }];
        prune_records(&mut records, &[], now, None);
        let payload: ResourceHistoryPayload = serde_json::from_slice(&records[0].payload).unwrap();
        assert!(matches!(
            payload,
            ResourceHistoryPayload::CaptureGap { gap: result, .. }
                if result.reason_code == "journal_full"
        ));
    }

    #[test]
    fn resource_retention_uses_absolute_age_windows() {
        const DAY: u64 = 24 * 60 * 60;
        assert_eq!(
            resource_resolution_for_age(14 * DAY),
            Some(crate::state::history_repository::replica::RetentionResolution::Minute)
        );
        assert_eq!(
            resource_resolution_for_age(90 * DAY),
            Some(crate::state::history_repository::replica::RetentionResolution::FiveMinutes)
        );
        assert_eq!(
            resource_resolution_for_age(365 * DAY),
            Some(crate::state::history_repository::replica::RetentionResolution::Hour)
        );
        assert_eq!(resource_resolution_for_age(366 * DAY), None);
    }
}
