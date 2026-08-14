use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, VecDeque};

use crate::state::history_repository::replica::RepositoryReplicaGap;

use super::*;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct LocalSourceState {
    #[serde(default)]
    epoch: u64,
    #[serde(default)]
    streams: BTreeMap<String, LocalSourceStreamState>,
    #[serde(default)]
    last_dynamic_relay_attempt_unix_seconds: Option<u64>,
    #[serde(default)]
    primary_failure_cycles: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    primary_failure_repository_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    backpressure_gaps: BTreeMap<String, LocalSourceGap>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct LocalSourceStreamState {
    #[serde(default)]
    next_sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_segment_hash: Option<[u8; 32]>,
    #[serde(default, skip_serializing_if = "VecDeque::is_empty")]
    pending: VecDeque<StoredSegment>,
    #[serde(default)]
    backpressured_records: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocalSourceGap {
    source_epoch: u64,
    first_sequence: u64,
    last_sequence: u64,
    start_unix_seconds: u64,
    end_unix_seconds: u64,
}

impl RepositoryReplicaRuntime {
    pub(crate) fn queue_local_source_segments(
        &mut self,
        cluster_id: &str,
        identity: RepositoryNodeIdentity,
        signing_key: &ed25519_dalek::SigningKey,
        records: Vec<SyncRecord>,
        now_unix_seconds: u64,
    ) -> Result<Vec<RepositoryReplicaSegment>, RepositoryRuntimeError> {
        let mut records_by_stream = BTreeMap::<&'static str, Vec<SyncRecord>>::new();
        for record in records {
            let (schema_id, _) = record.schema();
            let stream = stream_for_schema(schema_id).ok_or_else(|| {
                RepositoryRuntimeError::Storage(format!(
                    "history source schema has no independent stream: {schema_id}"
                ))
            })?;
            records_by_stream.entry(stream).or_default().push(record);
        }
        // Five streams at the protocol's 256 KiB segment ceiling stay below the 16 MiB state guard.
        const MAX_PENDING_SEGMENTS_PER_STREAM: usize = 8;
        if records_by_stream.is_empty() {
            return Ok(self.local_source_pending_segments());
        }
        if self.snapshot.local_source.epoch == 0 {
            self.snapshot.local_source.epoch =
                source_epoch(cluster_id, identity.node_id().as_str(), now_unix_seconds);
        }
        for (stream, records) in records_by_stream {
            let record_count = u64::try_from(records.len())
                .map_err(|_| RepositoryRuntimeError::StateLimitExceeded)?;
            let (pending_segments, backpressured_records, sequence) = {
                let stream_state = self
                    .snapshot
                    .local_source
                    .streams
                    .entry(stream.to_owned())
                    .or_default();
                if stream_state.pending.len() >= MAX_PENDING_SEGMENTS_PER_STREAM {
                    stream_state.backpressured_records = stream_state
                        .backpressured_records
                        .saturating_add(record_count);
                    (
                        Some(stream_state.pending.len()),
                        stream_state.backpressured_records,
                        stream_state.next_sequence,
                    )
                } else {
                    (None, 0, 0)
                }
            };
            if let Some(pending_segments) = pending_segments {
                self.record_local_source_backpressure_gap(stream, sequence, now_unix_seconds);
                tracing::warn!(
                    stream,
                    pending_segments,
                    backpressured_records,
                    "history source outbox is backpressured"
                );
                continue;
            }
            let stream_state = self
                .snapshot
                .local_source
                .streams
                .get_mut(stream)
                .expect("source stream was initialized");
            let first_cursor = Cursor::new(
                identity.node_id().as_str(),
                self.snapshot.local_source.epoch,
                stream,
                stream_state.next_sequence,
            )?;
            let signed = CanonicalSegment::new(
                cluster_id,
                first_cursor,
                records,
                stream_state.previous_segment_hash,
                now_unix_seconds,
                now_unix_seconds,
            )?
            .sign(signing_key)?;
            let wire = signed.wire_bytes()?;
            stream_state.next_sequence = stream_state
                .next_sequence
                .checked_add(record_count)
                .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
            stream_state.previous_segment_hash = Some(signed.segment_hash()?);
            stream_state.pending.push_back(StoredSegment {
                id: hex::encode(Sha256::digest(&wire)),
                identity: identity.clone(),
                wire: wire.clone(),
            });
        }
        self.persist_control_state()?;
        Ok(self.local_source_pending_segments())
    }

    pub(crate) fn queue_local_source_segment(
        &mut self,
        cluster_id: &str,
        identity: RepositoryNodeIdentity,
        signing_key: &ed25519_dalek::SigningKey,
        records: Vec<SyncRecord>,
        now_unix_seconds: u64,
    ) -> Result<Option<RepositoryReplicaSegment>, RepositoryRuntimeError> {
        Ok(self
            .queue_local_source_segments(
                cluster_id,
                identity,
                signing_key,
                records,
                now_unix_seconds,
            )?
            .into_iter()
            .next())
    }

    pub(crate) fn acknowledge_local_source_segment(
        &mut self,
        delivered_wire: &[u8],
    ) -> Result<(), RepositoryRuntimeError> {
        let mut acknowledged = false;
        for stream in self.snapshot.local_source.streams.values_mut() {
            if stream
                .pending
                .front()
                .is_some_and(|pending| pending.wire == delivered_wire)
            {
                stream.pending.pop_front();
                acknowledged = true;
            }
        }
        if acknowledged {
            self.persist_control_state()?;
        }
        Ok(())
    }

    pub(crate) fn begin_source_dynamic_relay_attempt(
        &mut self,
        cluster_id: &str,
        now_unix_seconds: u64,
    ) -> Result<bool, RepositoryRuntimeError> {
        let jitter_seconds = u64::from(Sha256::digest(cluster_id.as_bytes())[1]) % (5 * 60);
        let due = self
            .snapshot
            .local_source
            .last_dynamic_relay_attempt_unix_seconds
            .is_none_or(|last| now_unix_seconds.saturating_sub(last) >= 60 * 60 + jitter_seconds);
        if due {
            self.snapshot
                .local_source
                .last_dynamic_relay_attempt_unix_seconds = Some(now_unix_seconds);
            self.persist_control_state()?;
        }
        Ok(due)
    }

    pub(crate) fn local_source_pending_segments(&self) -> Vec<RepositoryReplicaSegment> {
        self.snapshot
            .local_source
            .streams
            .values()
            .filter_map(|stream| stream.pending.front())
            .map(|pending| RepositoryReplicaSegment {
                identity: pending.identity.clone(),
                wire: pending.wire.clone(),
            })
            .collect()
    }

    pub(crate) fn local_source_backpressure_gaps(
        &self,
        source_node_id: &str,
    ) -> Vec<RepositoryReplicaGap> {
        self.snapshot
            .local_source
            .backpressure_gaps
            .iter()
            .map(|(stream, gap)| RepositoryReplicaGap {
                source_node_id: source_node_id.to_owned(),
                source_epoch: gap.source_epoch,
                stream: stream.clone(),
                first_sequence: gap.first_sequence,
                last_sequence: gap.last_sequence,
                start_unix_seconds: gap.start_unix_seconds,
                end_unix_seconds: gap.end_unix_seconds,
                permanent: true,
            })
            .collect()
    }

    pub(crate) fn local_source_collector(
        &self,
        primary_repository_id: &str,
        standby_repository_id: Option<&str>,
    ) -> String {
        if self
            .snapshot
            .local_source
            .primary_failure_repository_id
            .as_deref()
            == Some(primary_repository_id)
            && self.snapshot.local_source.primary_failure_cycles >= 3
            && let Some(standby_repository_id) = standby_repository_id
        {
            return standby_repository_id.to_owned();
        }
        primary_repository_id.to_owned()
    }

    pub(crate) fn record_local_source_collector_delivery(
        &mut self,
        primary_repository_id: &str,
        selected_repository_id: &str,
        succeeded: bool,
    ) -> Result<(), RepositoryRuntimeError> {
        if self
            .snapshot
            .local_source
            .primary_failure_repository_id
            .as_deref()
            != Some(primary_repository_id)
        {
            self.snapshot.local_source.primary_failure_repository_id =
                Some(primary_repository_id.to_owned());
            self.snapshot.local_source.primary_failure_cycles = 0;
        }
        if selected_repository_id != primary_repository_id {
            return self.persist_control_state();
        }
        if succeeded {
            self.snapshot.local_source.primary_failure_cycles = 0;
        } else {
            self.snapshot.local_source.primary_failure_cycles = self
                .snapshot
                .local_source
                .primary_failure_cycles
                .saturating_add(1);
        }
        self.persist_control_state()
    }

    fn record_local_source_backpressure_gap(
        &mut self,
        stream: &str,
        sequence: u64,
        now_unix_seconds: u64,
    ) {
        let epoch = self.snapshot.local_source.epoch;
        let gap = self
            .snapshot
            .local_source
            .backpressure_gaps
            .entry(stream.to_owned())
            .or_insert(LocalSourceGap {
                source_epoch: epoch,
                first_sequence: sequence,
                last_sequence: sequence,
                start_unix_seconds: now_unix_seconds,
                end_unix_seconds: now_unix_seconds,
            });
        gap.first_sequence = gap.first_sequence.min(sequence);
        gap.last_sequence = gap.last_sequence.max(sequence);
        gap.start_unix_seconds = gap.start_unix_seconds.min(now_unix_seconds);
        gap.end_unix_seconds = gap.end_unix_seconds.max(now_unix_seconds);
    }
}

fn stream_for_schema(schema_id: &str) -> Option<&'static str> {
    Some(match schema_id {
        "runtime.v1" => "runtime",
        "path_health.v1" => "path_health",
        "traffic.v1" => "traffic",
        "connections.v1" => "connections",
        "ip_usage.v1" => "ip_usage",
        "tombstone.v1" => "tombstone",
        _ => return None,
    })
}

fn source_epoch(cluster_id: &str, node_id: &str, now_unix_seconds: u64) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(b"xp-history-source-epoch-v1\0");
    hasher.update(cluster_id.as_bytes());
    hasher.update([0]);
    hasher.update(node_id.as_bytes());
    hasher.update([0]);
    hasher.update(now_unix_seconds.to_be_bytes());
    let bytes: [u8; 32] = hasher.finalize().into();
    u64::from_be_bytes(bytes[..8].try_into().expect("SHA-256 prefix")).max(1)
}
