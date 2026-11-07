use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signer as _, Verifier as _};

mod provenance;
use provenance::verify_record;
pub(super) use provenance::{attest_record, record_key};

const DOMAIN: &[u8] = b"xp-source-prefix-recovery-v1\0";
const MAX_BUNDLE_BYTES: usize = 192 * 1024;
const MAX_LEDGER_BYTES: usize = 1024 * 1024;
pub(crate) const LOSS_REASON: &str = "operator_confirmed_unrecoverable_prefix";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PrefixRecoveryPlan {
    pub(crate) stream: String,
    pub(crate) source_epoch: u64,
    pub(crate) receiver_watermark: u64,
    pub(crate) head_segment_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PrefixRecoveryStream {
    pub(crate) plan: PrefixRecoveryPlan,
    #[serde(with = "super::base64_bytes")]
    pub(crate) original_head: Vec<u8>,
    pub(crate) retained_records: Vec<StoredRecord>,
    pub(crate) retained_record_signatures: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PrefixRecoveryBundle {
    pub(crate) version: u8,
    pub(crate) cluster_id: String,
    pub(crate) collector_node_id: String,
    pub(crate) identity: RepositoryNodeIdentity,
    pub(crate) cluster_binding: String,
    pub(crate) source_deletion_version: u64,
    #[serde(with = "super::base64_bytes")]
    pub(crate) deletion_baseline_wire: Vec<u8>,
    pub(crate) streams: Vec<PrefixRecoveryStream>,
    pub(crate) signature: String,
}

/// Evidence has its own bounded ledger. Overlapping aggregate contributions must never enter
/// the ordinary retention reducer, whose addition would count common samples twice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PrefixRecoveryEvidence {
    pub(crate) bundle_digest: String,
    pub(crate) applied_at_unix_seconds: u64,
    pub(crate) identity: RepositoryNodeIdentity,
    pub(crate) source_signature: String,
    pub(crate) retained_records: Vec<StoredRecord>,
    pub(crate) retained_record_signatures: BTreeMap<String, String>,
    pub(crate) gaps: Vec<RepositoryReplicaGap>,
}

fn invalid(message: &str) -> RepositoryRuntimeError {
    RepositoryRuntimeError::Storage(message.to_owned())
}

impl PrefixRecoveryBundle {
    fn signing_bytes(&self) -> Result<Vec<u8>, RepositoryRuntimeError> {
        let mut unsigned = self.clone();
        unsigned.signature.clear();
        // Per-record proofs bind this inventory digest; excluding the proofs avoids a cycle.
        for stream in &mut unsigned.streams {
            stream.retained_record_signatures.clear();
        }
        let mut bytes = DOMAIN.to_vec();
        bytes.extend(serde_json::to_vec(&unsigned).map_err(|error| invalid(&error.to_string()))?);
        if bytes.len() > MAX_BUNDLE_BYTES {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        Ok(bytes)
    }

    pub(crate) fn digest(&self) -> Result<String, RepositoryRuntimeError> {
        Ok(hex::encode(Sha256::digest(self.signing_bytes()?)))
    }

    pub(crate) fn verify(&self) -> Result<(), RepositoryRuntimeError> {
        if self.version != 1 || self.streams.is_empty() || self.streams.len() > 8 {
            return Err(invalid(
                "invalid prefix recovery bundle version or stream count",
            ));
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(&self.signature)
            .map_err(|_| invalid("invalid prefix recovery signature encoding"))?;
        let signature = ed25519_dalek::Signature::from_slice(&bytes)
            .map_err(|_| invalid("invalid prefix recovery signature"))?;
        let key =
            ed25519_dalek::VerifyingKey::from_bytes(self.identity.ed25519_public_key().as_bytes())
                .map_err(|_| invalid("invalid prefix recovery public key"))?;
        key.verify(&Sha256::digest(self.signing_bytes()?), &signature)
            .map_err(|_| invalid("prefix recovery source signature mismatch"))?;
        if serde_json::to_vec(self)
            .map_err(|error| invalid(&error.to_string()))?
            .len()
            > MAX_BUNDLE_BYTES
        {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        let digest = self.digest()?;
        let mut streams = BTreeSet::new();
        let mut count = 0_usize;
        if !self.deletion_baseline_wire.is_empty() {
            let baseline = SignedSegment::from_wire(&self.deletion_baseline_wire)?;
            baseline.verify_identity(&self.identity)?;
            let cursor = baseline.canonical().first_cursor();
            if baseline.canonical().cluster_id() != self.cluster_id
                || cursor.source_node_id() != self.identity.node_id().as_str()
                || !cursor.stream().starts_with("tombstone-prefix-recovery-")
                || cursor.sequence() != 0
                || baseline.canonical().records().is_empty()
                || baseline.canonical().records().iter().any(|record| {
                    !record.is_tombstone()
                        || !record.record_key().ends_with(b":")
                        || !is_known_schema(record)
                })
            {
                return Err(invalid("invalid prefix recovery deletion baseline"));
            }
            count = baseline.canonical().records().len();
            if count > 128 {
                return Err(RepositoryRuntimeError::StateLimitExceeded);
            }
        }
        for stream in &self.streams {
            let plan = &stream.plan;
            if !matches!(plan.stream.as_str(), "runtime" | "traffic" | "path_health")
                || !streams.insert((plan.source_epoch, plan.stream.clone()))
            {
                return Err(invalid(
                    "prefix recovery requires distinct node-history streams",
                ));
            }
            let head = SignedSegment::from_wire(&stream.original_head)?;
            head.verify_identity(&self.identity)?;
            let cursor = head.canonical().first_cursor();
            if head.canonical().cluster_id() != self.cluster_id
                || cursor.source_node_id() != self.identity.node_id().as_str()
                || cursor.source_epoch() != plan.source_epoch
                || cursor.stream() != plan.stream
                || hex::encode(Sha256::digest(&stream.original_head)) != plan.head_segment_id
                || plan
                    .receiver_watermark
                    .checked_add(1)
                    .is_none_or(|first| first >= cursor.sequence())
            {
                return Err(invalid(
                    "prefix recovery head does not bind the missing range",
                ));
            }
            if stream.retained_record_signatures.len() != stream.retained_records.len() {
                return Err(invalid("prefix aggregate proofs are incomplete"));
            }
            let mut keys = BTreeSet::new();
            for record in &stream.retained_records {
                let signature = stream
                    .retained_record_signatures
                    .get(&record_key(record)?)
                    .ok_or_else(|| invalid("prefix aggregate proof missing"))?;
                verify_record(&self.identity, &digest, record, signature)?;
                count += 1;
                if count > 128
                    || record.tombstone
                    || record.source_node_id != cursor.source_node_id()
                    || record.source_epoch != cursor.source_epoch()
                    || record.stream != cursor.stream()
                    || source_stream_for_schema(&record.schema_id) != Some(plan.stream.as_str())
                    || record.schema_version != 1
                    || record.sequence <= plan.receiver_watermark
                    || record.sequence >= cursor.sequence()
                    || retention::aggregate_metadata(record).is_none()
                    || !keys.insert((
                        record.sequence,
                        record.subject_node_id.clone(),
                        record.observer_node_id.clone(),
                        record.record_key.clone(),
                    ))
                {
                    return Err(invalid(
                        "prefix recovery retained aggregate inventory is invalid",
                    ));
                }
            }
        }
        Ok(())
    }
}

impl RepositoryReplicaRuntime {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn export_prefix_recovery(
        &self,
        cluster_id: &str,
        collector_node_id: &str,
        identity: RepositoryNodeIdentity,
        key: &ed25519_dalek::SigningKey,
        cluster_binding: String,
        plans: Vec<PrefixRecoveryPlan>,
        deletion_version: u64,
        deletions: &[crate::node_history::RepositoryHistoryDeletionMarker],
    ) -> Result<PrefixRecoveryBundle, RepositoryRuntimeError> {
        if plans.is_empty() || plans.len() > 8 {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        let mut streams = Vec::new();
        for plan in plans {
            let rows = self
                .storage
                .source_delivery_journal_by_ids(
                    std::slice::from_ref(&plan.head_segment_id),
                    1024 * 1024,
                )
                .map_err(|error| invalid(&error.to_string()))?;
            let row = rows
                .into_iter()
                .next()
                .ok_or_else(|| invalid("missing prefix head"))?;
            let signed = SignedSegment::from_wire(&row.wire)?;
            let cursor = signed.canonical().first_cursor();
            if row.identity != identity
                || !self
                    .storage
                    .prefix_recovery_head_is_first(
                        identity.node_id().as_str(),
                        plan.source_epoch,
                        &plan.stream,
                        cursor.sequence(),
                    )
                    .map_err(|error| invalid(&error.to_string()))?
            {
                return Err(invalid(
                    "prefix head is not the first durable pending stream row",
                ));
            }
            let first = plan
                .receiver_watermark
                .checked_add(1)
                .ok_or_else(|| invalid("prefix watermark overflow"))?;
            let last = cursor
                .sequence()
                .checked_sub(1)
                .ok_or_else(|| invalid("prefix head cannot be zero"))?;
            if first > last {
                return Err(invalid("prefix recovery requires a nonempty missing range"));
            }
            let records = self
                .storage
                .prefix_recovery_records(
                    identity.node_id().as_str(),
                    plan.source_epoch,
                    &plan.stream,
                    first,
                    last,
                )
                .map_err(|error| invalid(&error.to_string()))?
                .into_iter()
                .map(StoredRecord::from_sqlite_row)
                .collect::<Result<Vec<_>, _>>()?;
            streams.push(PrefixRecoveryStream {
                plan,
                original_head: row.wire,
                retained_records: records,
                retained_record_signatures: BTreeMap::new(),
            });
        }
        let mut bundle = PrefixRecoveryBundle {
            version: 1,
            cluster_id: cluster_id.to_owned(),
            collector_node_id: collector_node_id.to_owned(),
            identity,
            cluster_binding,
            source_deletion_version: deletion_version,
            deletion_baseline_wire: Vec::new(),
            streams,
            signature: String::new(),
        };
        if !deletions.is_empty() {
            if deletions.len() > 128 {
                return Err(RepositoryRuntimeError::StateLimitExceeded);
            }
            let records = deletions
                .iter()
                .map(|marker| {
                    let target = marker
                        .target_node_id()
                        .unwrap_or(bundle.identity.node_id().as_str());
                    SyncRecord::new(
                        target,
                        target,
                        &marker.schema_id,
                        1,
                        marker.record_key.clone(),
                        Vec::new(),
                        true,
                    )
                })
                .collect::<Vec<_>>();
            let binding = serde_json::to_vec(&(
                &bundle.streams,
                deletion_version,
                &records
                    .iter()
                    .map(|record| (record.schema(), record.record_key()))
                    .collect::<Vec<_>>(),
            ))
            .map_err(|error| invalid(&error.to_string()))?;
            let stream = format!(
                "tombstone-prefix-recovery-{}",
                &hex::encode(Sha256::digest(binding))[..32]
            );
            bundle.deletion_baseline_wire = CanonicalSegment::new(
                cluster_id,
                Cursor::new(
                    bundle.identity.node_id().as_str(),
                    bundle.streams[0].plan.source_epoch,
                    &stream,
                    0,
                )?,
                records,
                None,
                0,
                0,
            )?
            .sign(key)?
            .wire_bytes()?;
        }
        bundle.signature = URL_SAFE_NO_PAD.encode(
            key.sign(&Sha256::digest(bundle.signing_bytes()?))
                .to_bytes(),
        );
        let digest = bundle.digest()?;
        for stream in &mut bundle.streams {
            for record in &stream.retained_records {
                stream
                    .retained_record_signatures
                    .insert(record_key(record)?, attest_record(&digest, record, key)?);
            }
        }
        bundle.verify()?;
        Ok(bundle)
    }

    pub(crate) fn prefix_recovery_fingerprint(
        &self,
        bundle: &PrefixRecoveryBundle,
    ) -> Result<String, RepositoryRuntimeError> {
        bundle.verify()?;
        let mut digest = Sha256::new();
        digest.update(bundle.digest()?.as_bytes());
        digest.update(
            serde_json::to_vec(&self.tombstones.checkpoint())
                .map_err(|error| invalid(&error.to_string()))?,
        );
        for stream in &bundle.streams {
            let head = SignedSegment::from_wire(&stream.original_head)?;
            let cursor = head.canonical().first_cursor();
            let watermark = self
                .receiver
                .as_ref()
                .map(|receiver| receiver.continuous_watermark_with_hash(cursor))
                .transpose()?
                .flatten();
            digest.update(
                serde_json::to_vec(&watermark).map_err(|error| invalid(&error.to_string()))?,
            );
            if watermark.as_ref().map(|(tip, _)| tip.sequence())
                != Some(stream.plan.receiver_watermark)
            {
                return Err(invalid("prefix recovery receiver watermark changed"));
            }
        }
        Ok(hex::encode(digest.finalize()))
    }

    pub(super) fn all_prefix_aware_gaps(&self) -> impl Iterator<Item = RepositoryReplicaGap> + '_ {
        self.snapshot
            .gaps
            .iter()
            .map(super::sync::gap_summary)
            .chain(
                self.snapshot
                    .prefix_recoveries
                    .iter()
                    .flat_map(|entry| entry.gaps.clone()),
            )
    }

    pub(crate) fn prefix_recovery_evidence(&self) -> &[PrefixRecoveryEvidence] {
        &self.snapshot.prefix_recoveries
    }

    /// Archive, explicit permanent gap, original signed head acceptance and consumption commit
    /// together. The source is never edited here: its ordinary delivery must obtain real ACKs.
    pub(crate) fn apply_prefix_recovery(
        &mut self,
        bundle: &PrefixRecoveryBundle,
        expected_fingerprint: &str,
        now: u64,
        ready: &[String],
        collector: &str,
    ) -> Result<Vec<RepositorySyncReceipt>, RepositoryRuntimeError> {
        if bundle.collector_node_id != collector || !ready.iter().any(|id| id == collector) {
            return Err(invalid("prefix recovery collector is not Ready"));
        }
        if self
            .snapshot
            .last_verified_unix_seconds
            .is_some_and(|verified| {
                ReplicaFreshness::new(verified, TOMBSTONE_HORIZON_SECONDS).requires_rebuild(now)
            })
        {
            return Err(invalid(
                "prefix recovery requires a fresh existing receiver",
            ));
        }
        if self.prefix_recovery_fingerprint(bundle)? != expected_fingerprint {
            return Err(invalid("prefix recovery fingerprint changed"));
        }
        self.bind_cluster(&bundle.cluster_id)?;
        self.ensure_receiver()?;
        let previous = self.snapshot.clone();
        let checkpoint = self.receiver.as_ref().expect("receiver").checkpoint()?;
        let result = (|| {
            let mut evidence = PrefixRecoveryEvidence {
                bundle_digest: bundle.digest()?,
                applied_at_unix_seconds: now,
                identity: bundle.identity.clone(),
                source_signature: bundle.signature.clone(),
                retained_records: Vec::new(),
                retained_record_signatures: BTreeMap::new(),
                gaps: Vec::new(),
            };
            for stream in &bundle.streams {
                let head = SignedSegment::from_wire(&stream.original_head)?;
                let first = head.canonical().first_cursor();
                // Missing originals cannot prove their observation-time envelope. Report a
                // conservative loss interval rather than inventing exact temporal coverage.
                let start = 0;
                let end = now;
                evidence.gaps.push(RepositoryReplicaGap {
                    source_node_id: first.source_node_id().to_owned(),
                    source_epoch: first.source_epoch(),
                    stream: first.stream().to_owned(),
                    first_sequence: stream.plan.receiver_watermark + 1,
                    last_sequence: first.sequence() - 1,
                    start_unix_seconds: start,
                    end_unix_seconds: end,
                    permanent: true,
                    reason: Some(LOSS_REASON.to_owned()),
                });
                for record in &stream.retained_records {
                    let cursor = ReplicaCursor::new(
                        &record.source_node_id,
                        record.source_epoch,
                        &record.stream,
                        record.sequence,
                    )?;
                    let key = ReplicaRecord::new(
                        &cursor,
                        &record.subject_node_id,
                        &record.observer_node_id,
                        &record.schema_id,
                        record.schema_version,
                        record.record_key.clone(),
                        record.payload.clone(),
                    )?
                    .key();
                    if self.tombstones.allows(&key) {
                        let key = record_key(record)?;
                        evidence
                            .retained_record_signatures
                            .insert(key.clone(), stream.retained_record_signatures[&key].clone());
                        evidence.retained_records.push(record.clone());
                    }
                }
            }
            evidence.verify()?;
            self.snapshot.prefix_recoveries.push(evidence.clone());
            if self.snapshot.prefix_recoveries.len() > 32
                || serde_json::to_vec(&self.snapshot.prefix_recoveries)
                    .map_err(|error| invalid(&error.to_string()))?
                    .len()
                    > MAX_LEDGER_BYTES
            {
                return Err(RepositoryRuntimeError::StateLimitExceeded);
            }
            // Keep the existing gap ledger's rotation policy; the dedicated evidence remains
            // durable and is separately included in query/convergence loss reporting.
            self.merge_replica_gaps_in_memory(&evidence.gaps)?;
            self.snapshot.history_truncated = true;
            let mut wires = bundle
                .streams
                .iter()
                .map(|stream| (stream.original_head.clone(), now))
                .collect::<Vec<_>>();
            if !bundle.deletion_baseline_wire.is_empty() {
                wires.insert(0, (bundle.deletion_baseline_wire.clone(), now));
            }
            self.receive_repair_wires_with_receipts(
                &bundle.cluster_id,
                &bundle.identity,
                &wires,
                ready,
                collector,
            )
        })();
        if result.is_err() {
            self.restore(&checkpoint, previous)?;
        }
        result
    }
}
