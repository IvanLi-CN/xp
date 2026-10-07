use super::{RepositoryReplicaRuntime, RepositoryRuntimeError};
use crate::history_sync::Cursor;
use crate::state::history_repository::control::{
    HISTORY_RECOVERY_METADATA_BUDGET_BYTES, HISTORY_RECOVERY_PAGE_BUDGET_BYTES,
    HISTORY_REPOSITORY_LOW_SPACE_GUARD_BYTES,
};
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub(crate) struct RetainedAnchorCheckpointUpdate {
    pub(crate) peer_node_id: String,
    pub(crate) response_id: String,
    pub(crate) response_complete: bool,
    pub(crate) allowance_complete: bool,
    pub(crate) streams: BTreeSet<InitialPeerRetainedAnchorStream>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InitialPeerBackfillCheckpoint {
    #[serde(default)]
    pub(crate) page_cursor: Option<String>,
    #[serde(default)]
    pub(crate) stream_state: BTreeMap<String, (u64, Option<[u8; 32]>)>,
    #[serde(default)]
    pub(crate) saw_history: bool,
    #[serde(default)]
    pub(crate) completed: bool,
    #[serde(default)]
    pub(crate) epoch: u64,
    #[serde(default)]
    pub(crate) summary_cursor: Option<String>,
    #[serde(default)]
    pub(crate) summary_pending_segment_ids: Vec<String>,
    #[serde(default)]
    pub(crate) summary_pending_next_cursor: Option<String>,
    #[serde(default)]
    pub(crate) summary_complete: bool,
    #[serde(default)]
    pub(crate) summary_requires_tiered_backfill: bool,
    #[serde(default)]
    pub(crate) retained_anchor_repair_response_seen: bool,
    #[serde(default)]
    pub(crate) retained_anchor_repair_response_id: Option<String>,
    #[serde(default)]
    pub(crate) retained_anchor_streams: BTreeSet<InitialPeerRetainedAnchorStream>,
    /// Completed tiered handoffs remain durable even after the bounded gap ledger rotates.
    #[serde(default)]
    pub(crate) retained_anchor_handoffs: BTreeSet<InitialPeerTieredHandoff>,
    #[serde(default)]
    pub(crate) summary_tiered_handoff: Option<InitialPeerTieredHandoff>,
    /// Explicit operator-authorized recovery generation. A generation may cross one retained
    /// prefix only once; legacy completed handoffs never implicitly authorize another crossing.
    #[serde(default)]
    pub(crate) recovery_generation: u64,
    #[serde(default)]
    pub(crate) recovery_generation_consumed: bool,
    #[serde(default)]
    pub(crate) recovery_handoff: Option<InitialPeerTieredHandoff>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct InitialPeerRetainedAnchorStream {
    pub(crate) source_node_id: String,
    pub(crate) source_epoch: u64,
    pub(crate) stream: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct InitialPeerTieredHandoff {
    pub(crate) source_node_id: String,
    pub(crate) source_epoch: u64,
    pub(crate) stream: String,
    pub(crate) first_missing: u64,
    pub(crate) last_missing: u64,
    pub(crate) next_sequence: u64,
    pub(crate) end_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InitialPeerRecoveryPreview {
    pub(crate) peer_node_id: String,
    pub(crate) generation: u64,
    pub(crate) receiver_watermark: Option<u64>,
    pub(crate) previous_handoff: Option<InitialPeerTieredHandoff>,
    pub(crate) capacity_quota_bytes: u64,
    pub(crate) capacity_used_bytes: u64,
    pub(crate) capacity_available_bytes: u64,
    pub(crate) capacity_required_bytes: u64,
    pub(crate) capacity_filesystem_required_bytes: u64,
    pub(crate) capacity_quota_shortfall_bytes: u64,
    pub(crate) capacity_filesystem_shortfall_bytes: u64,
    pub(crate) fingerprint: String,
}

impl RepositoryReplicaRuntime {
    pub(crate) fn initial_peer_backfill_checkpoint(
        &self,
        peer_node_id: &str,
    ) -> Option<InitialPeerBackfillCheckpoint> {
        self.snapshot
            .initial_peer_backfills
            .get(peer_node_id)
            .cloned()
    }

    pub(crate) fn update_initial_peer_backfill_checkpoint(
        &mut self,
        peer_node_id: &str,
        page_cursor: Option<String>,
        stream_state: BTreeMap<String, (u64, Option<[u8; 32]>)>,
        saw_history: bool,
        completed: bool,
    ) -> Result<(), RepositoryRuntimeError> {
        let previous_snapshot = self.snapshot.clone();
        let checkpoint = self
            .snapshot
            .initial_peer_backfills
            .get(peer_node_id)
            .cloned()
            .unwrap_or_default();
        self.snapshot.initial_peer_backfills.insert(
            peer_node_id.to_owned(),
            InitialPeerBackfillCheckpoint {
                page_cursor,
                stream_state,
                saw_history,
                completed,
                epoch: checkpoint.epoch,
                summary_cursor: checkpoint.summary_cursor,
                summary_pending_segment_ids: checkpoint.summary_pending_segment_ids,
                summary_pending_next_cursor: checkpoint.summary_pending_next_cursor,
                summary_complete: checkpoint.summary_complete,
                summary_requires_tiered_backfill: checkpoint.summary_requires_tiered_backfill,
                retained_anchor_repair_response_seen: checkpoint
                    .retained_anchor_repair_response_seen,
                retained_anchor_repair_response_id: checkpoint.retained_anchor_repair_response_id,
                retained_anchor_streams: checkpoint.retained_anchor_streams,
                retained_anchor_handoffs: checkpoint.retained_anchor_handoffs,
                summary_tiered_handoff: checkpoint.summary_tiered_handoff,
                recovery_generation: checkpoint.recovery_generation,
                recovery_generation_consumed: checkpoint.recovery_generation_consumed,
                recovery_handoff: checkpoint.recovery_handoff,
            },
        );
        if let Err(error) = self.persist_control_state() {
            self.snapshot = previous_snapshot;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn update_initial_peer_summary_checkpoint(
        &mut self,
        peer_node_id: &str,
        summary_cursor: Option<String>,
        pending_segment_ids: Vec<String>,
        pending_next_cursor: Option<String>,
        summary_complete: bool,
        summary_requires_tiered_backfill: bool,
    ) -> Result<(), RepositoryRuntimeError> {
        let previous_snapshot = self.snapshot.clone();
        let checkpoint = self
            .snapshot
            .initial_peer_backfills
            .entry(peer_node_id.to_owned())
            .or_default();
        checkpoint.summary_cursor = summary_cursor;
        checkpoint.summary_pending_segment_ids = pending_segment_ids;
        checkpoint.summary_pending_next_cursor = pending_next_cursor;
        checkpoint.summary_complete = summary_complete;
        checkpoint.summary_requires_tiered_backfill = summary_requires_tiered_backfill;
        if let Err(error) = self.persist_control_state() {
            self.snapshot = previous_snapshot;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn clear_initial_peer_retained_anchor_response_id(
        &mut self,
        peer_node_id: &str,
    ) -> Result<(), RepositoryRuntimeError> {
        let previous_snapshot = self.snapshot.clone();
        let Some(checkpoint) = self.snapshot.initial_peer_backfills.get_mut(peer_node_id) else {
            return Ok(());
        };
        if checkpoint
            .retained_anchor_repair_response_id
            .take()
            .is_some()
            && let Err(error) = self.persist_control_state()
        {
            self.snapshot = previous_snapshot;
            return Err(error);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update_initial_peer_summary_checkpoint_with_retained_anchors(
        &mut self,
        peer_node_id: &str,
        summary_cursor: Option<String>,
        pending_segment_ids: Vec<String>,
        pending_next_cursor: Option<String>,
        summary_complete: bool,
        summary_requires_tiered_backfill: bool,
        retained_anchor_repair_response_seen: bool,
        retained_anchor_streams: BTreeSet<InitialPeerRetainedAnchorStream>,
    ) -> Result<(), RepositoryRuntimeError> {
        let response_id = self
            .snapshot
            .initial_peer_backfills
            .get(peer_node_id)
            .and_then(|checkpoint| checkpoint.retained_anchor_repair_response_id.clone());
        self.update_initial_peer_summary_checkpoint_with_retained_anchor_response(
            peer_node_id,
            summary_cursor,
            pending_segment_ids,
            pending_next_cursor,
            summary_complete,
            summary_requires_tiered_backfill,
            response_id,
            retained_anchor_repair_response_seen,
            retained_anchor_repair_response_seen,
            retained_anchor_streams,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update_initial_peer_summary_checkpoint_with_retained_anchor_response(
        &mut self,
        peer_node_id: &str,
        summary_cursor: Option<String>,
        pending_segment_ids: Vec<String>,
        pending_next_cursor: Option<String>,
        summary_complete: bool,
        summary_requires_tiered_backfill: bool,
        retained_anchor_repair_response_id: Option<String>,
        retained_anchor_repair_response_complete: bool,
        retained_anchor_allowance_complete: bool,
        retained_anchor_streams: BTreeSet<InitialPeerRetainedAnchorStream>,
    ) -> Result<(), RepositoryRuntimeError> {
        let previous_snapshot = self.snapshot.clone();
        let checkpoint = self
            .snapshot
            .initial_peer_backfills
            .entry(peer_node_id.to_owned())
            .or_default();
        match (
            checkpoint.retained_anchor_repair_response_id.as_deref(),
            retained_anchor_repair_response_id.as_deref(),
        ) {
            (Some(existing), Some(incoming)) if existing != incoming => {
                return Err(RepositoryRuntimeError::Storage(
                    "retained anchor repair response changed before completion".to_owned(),
                ));
            }
            (Some(_), None) => {
                return Err(RepositoryRuntimeError::Storage(
                    "retained anchor repair response identity is missing".to_owned(),
                ));
            }
            _ => {}
        }
        checkpoint.summary_cursor = summary_cursor;
        checkpoint.summary_pending_segment_ids = pending_segment_ids;
        checkpoint.summary_pending_next_cursor = pending_next_cursor;
        checkpoint.summary_complete = summary_complete;
        checkpoint.summary_requires_tiered_backfill = summary_requires_tiered_backfill;
        if retained_anchor_repair_response_complete {
            checkpoint.retained_anchor_repair_response_id = None;
        } else {
            checkpoint.retained_anchor_repair_response_id = retained_anchor_repair_response_id;
        }
        if retained_anchor_allowance_complete {
            checkpoint.retained_anchor_repair_response_seen = true;
            checkpoint.retained_anchor_repair_response_id = None;
        }
        checkpoint.retained_anchor_streams = retained_anchor_streams;
        if let Err(error) = self.persist_control_state() {
            self.snapshot = previous_snapshot;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn start_initial_peer_tiered_handoff(
        &mut self,
        peer_node_id: &str,
        handoff: InitialPeerTieredHandoff,
    ) -> Result<(), RepositoryRuntimeError> {
        let previous_snapshot = self.snapshot.clone();
        let prior = self
            .snapshot
            .initial_peer_backfills
            .get(peer_node_id)
            .cloned()
            .unwrap_or_default();
        self.snapshot.initial_peer_backfills.insert(
            peer_node_id.to_owned(),
            InitialPeerBackfillCheckpoint {
                page_cursor: None,
                stream_state: BTreeMap::new(),
                saw_history: false,
                completed: false,
                epoch: prior.epoch,
                summary_cursor: prior.summary_cursor,
                summary_pending_segment_ids: prior.summary_pending_segment_ids,
                summary_pending_next_cursor: prior.summary_pending_next_cursor,
                summary_complete: false,
                summary_requires_tiered_backfill: true,
                retained_anchor_repair_response_seen: prior.retained_anchor_repair_response_seen,
                retained_anchor_repair_response_id: prior.retained_anchor_repair_response_id,
                retained_anchor_streams: prior.retained_anchor_streams,
                retained_anchor_handoffs: prior.retained_anchor_handoffs,
                summary_tiered_handoff: Some(handoff),
                recovery_generation: prior.recovery_generation,
                recovery_generation_consumed: prior.recovery_generation > 0,
                recovery_handoff: prior.recovery_handoff,
            },
        );
        if let Err(error) = self.persist_control_state() {
            self.snapshot = previous_snapshot;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn complete_initial_peer_tiered_handoff(
        &mut self,
        peer_node_id: &str,
        handoff: &InitialPeerTieredHandoff,
    ) -> Result<(), RepositoryRuntimeError> {
        let checkpoint = self
            .snapshot
            .initial_peer_backfills
            .get(peer_node_id)
            .ok_or_else(|| {
                RepositoryRuntimeError::Storage("tiered handoff checkpoint is missing".to_owned())
            })?;
        if checkpoint.summary_tiered_handoff.as_ref() != Some(handoff) || !checkpoint.completed {
            return Err(RepositoryRuntimeError::Storage(
                "tiered handoff export is incomplete".to_owned(),
            ));
        }
        let next = Cursor::new(
            handoff.source_node_id.clone(),
            handoff.source_epoch,
            handoff.stream.clone(),
            handoff.next_sequence,
        )?;
        let previous_receiver = self
            .receiver
            .as_ref()
            .ok_or(RepositoryRuntimeError::ClusterBindingMismatch)?
            .checkpoint()?;
        let previous_snapshot = self.snapshot.clone();
        let gap = super::RepositoryReplicaGap {
            source_node_id: handoff.source_node_id.clone(),
            source_epoch: handoff.source_epoch,
            stream: handoff.stream.clone(),
            first_sequence: handoff.first_missing,
            last_sequence: handoff.last_missing,
            start_unix_seconds: 0,
            end_unix_seconds: handoff.end_unix_seconds,
            permanent: true,
            reason: Some("source_retention_expired".to_owned()),
        };
        self.merge_replica_gaps_in_memory(&[gap])?;
        let handoff_already_bridged = self
            .receiver
            .as_ref()
            .expect("receiver checked above")
            .continuous_watermark(&next)?
            .is_some_and(|watermark| watermark.sequence() >= handoff.next_sequence)
            && self.snapshot.gaps.iter().any(|gap| {
                gap.permanent
                    && gap.source_node_id == handoff.source_node_id
                    && gap.source_epoch == handoff.source_epoch
                    && gap.stream == handoff.stream
                    && gap.first_sequence <= handoff.first_missing
                    && gap.last_sequence >= handoff.last_missing
            });
        if !handoff_already_bridged {
            let advanced = self
                .receiver
                .as_mut()
                .expect("receiver checked above")
                .advance_declared_sequence_gap(
                    &next,
                    handoff.first_missing,
                    handoff.last_missing,
                )?;
            if !advanced {
                self.restore(&previous_receiver, previous_snapshot)?;
                return Err(RepositoryRuntimeError::Protocol(
                    crate::history_sync::ProtocolError::SequenceGap {
                        expected: handoff.first_missing,
                        actual: handoff.next_sequence,
                    },
                ));
            }
        }
        let checkpoint = self
            .snapshot
            .initial_peer_backfills
            .get_mut(peer_node_id)
            .expect("handoff checkpoint checked above");
        checkpoint.summary_tiered_handoff = None;
        checkpoint.retained_anchor_handoffs.insert(handoff.clone());
        if let Err(error) = self.persist_control_state() {
            self.restore(&previous_receiver, previous_snapshot)?;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn restart_initial_peer_backfill(
        &mut self,
        peer_node_id: &str,
    ) -> Result<(), RepositoryRuntimeError> {
        let previous_snapshot = self.snapshot.clone();
        let prior = self
            .snapshot
            .initial_peer_backfills
            .get(peer_node_id)
            .cloned()
            .unwrap_or_default();
        let epoch = prior.epoch;
        let retained_anchor_repair_response_seen = prior.retained_anchor_repair_response_seen;
        let retained_anchor_repair_response_id = prior.retained_anchor_repair_response_id;
        let retained_anchor_streams = prior.retained_anchor_streams;
        self.snapshot.initial_peer_backfills.insert(
            peer_node_id.to_owned(),
            InitialPeerBackfillCheckpoint {
                epoch,
                retained_anchor_repair_response_seen,
                retained_anchor_repair_response_id,
                retained_anchor_streams,
                retained_anchor_handoffs: prior.retained_anchor_handoffs,
                summary_tiered_handoff: prior.summary_tiered_handoff,
                recovery_generation: prior.recovery_generation,
                recovery_generation_consumed: prior.recovery_generation_consumed,
                recovery_handoff: prior.recovery_handoff,
                ..InitialPeerBackfillCheckpoint::default()
            },
        );
        if let Err(error) = self.persist_control_state() {
            self.snapshot = previous_snapshot;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn preview_initial_peer_recovery(
        &mut self,
        peer_node_id: &str,
    ) -> Result<InitialPeerRecoveryPreview, RepositoryRuntimeError> {
        let checkpoint = self
            .snapshot
            .initial_peer_backfills
            .get(peer_node_id)
            .cloned()
            .unwrap_or_default();
        if checkpoint.summary_tiered_handoff.is_some() {
            return Err(RepositoryRuntimeError::Storage(
                "history recovery already has an active tiered handoff".to_owned(),
            ));
        }
        let previous_handoff = checkpoint
            .recovery_handoff
            .clone()
            .or_else(|| checkpoint.retained_anchor_handoffs.iter().max().cloned());
        let Some(previous_handoff) = previous_handoff else {
            return Err(RepositoryRuntimeError::Storage(
                "history recovery requires a completed retained-anchor marker".to_owned(),
            ));
        };
        let receiver_watermark = self.receiver.as_ref().and_then(|receiver| {
            Cursor::new(
                previous_handoff.source_node_id.clone(),
                previous_handoff.source_epoch,
                previous_handoff.stream.clone(),
                previous_handoff.next_sequence,
            )
            .ok()
            .and_then(|cursor| receiver.continuous_watermark(&cursor).ok().flatten())
            .map(|cursor| cursor.sequence())
        });
        if receiver_watermark.is_none() {
            return Err(RepositoryRuntimeError::Storage(
                "history recovery requires a continuous receiver watermark".to_owned(),
            ));
        }
        if checkpoint.recovery_generation_consumed
            && checkpoint.recovery_handoff.as_ref().is_some_and(|handoff| {
                receiver_watermark == Some(handoff.first_missing.saturating_sub(1))
            })
        {
            return Err(RepositoryRuntimeError::Storage(
                "history recovery requires receiver watermark progress".to_owned(),
            ));
        }
        let capacity = self.runtime_capacity()?;
        let capacity_required_bytes = HISTORY_RECOVERY_PAGE_BUDGET_BYTES
            .saturating_add(HISTORY_RECOVERY_METADATA_BUDGET_BYTES);
        let capacity_filesystem_required_bytes =
            HISTORY_REPOSITORY_LOW_SPACE_GUARD_BYTES.saturating_add(capacity_required_bytes);
        let capacity_quota_available_bytes =
            capacity.quota_bytes().saturating_sub(capacity.used_bytes());
        let capacity_quota_shortfall_bytes =
            capacity_required_bytes.saturating_sub(capacity_quota_available_bytes);
        let capacity_filesystem_shortfall_bytes = capacity_filesystem_required_bytes
            .saturating_sub(capacity.filesystem_available_bytes());
        if !capacity
            .history_write_availability()
            .allows_history_writes()
            || capacity_quota_shortfall_bytes > 0
            || capacity_filesystem_shortfall_bytes > 0
        {
            return Err(RepositoryRuntimeError::Storage(format!(
                concat!(
                    "history recovery capacity preflight failed: ",
                    "required_bytes={}, ",
                    "quota_available_bytes={}, ",
                    "quota_shortfall_bytes={}, ",
                    "filesystem_required_bytes={}, ",
                    "filesystem_available_bytes={}, ",
                    "filesystem_shortfall_bytes={}"
                ),
                capacity_required_bytes,
                capacity_quota_available_bytes,
                capacity_quota_shortfall_bytes,
                capacity_filesystem_required_bytes,
                capacity.filesystem_available_bytes(),
                capacity_filesystem_shortfall_bytes
            )));
        }
        let generation =
            if checkpoint.recovery_generation > 0 && !checkpoint.recovery_generation_consumed {
                checkpoint.recovery_generation
            } else {
                checkpoint.recovery_generation.saturating_add(1).max(1)
            };
        let mut hasher = sha2::Sha256::new();
        hasher.update(b"xp-history-recovery-v1\0");
        hasher.update(peer_node_id.as_bytes());
        hasher.update(generation.to_be_bytes());
        hasher.update(receiver_watermark.unwrap_or_default().to_be_bytes());
        hasher.update(previous_handoff.source_node_id.as_bytes());
        hasher.update(previous_handoff.source_epoch.to_be_bytes());
        hasher.update(previous_handoff.stream.as_bytes());
        hasher.update(previous_handoff.first_missing.to_be_bytes());
        hasher.update(previous_handoff.last_missing.to_be_bytes());
        hasher.update(previous_handoff.next_sequence.to_be_bytes());
        hasher.update(previous_handoff.end_unix_seconds.to_be_bytes());
        hasher.update(capacity.quota_bytes().to_be_bytes());
        hasher.update(capacity.used_bytes().to_be_bytes());
        hasher.update(capacity.filesystem_available_bytes().to_be_bytes());
        hasher.update(capacity_required_bytes.to_be_bytes());
        hasher.update(capacity_filesystem_required_bytes.to_be_bytes());
        hasher.update(capacity_quota_shortfall_bytes.to_be_bytes());
        hasher.update(capacity_filesystem_shortfall_bytes.to_be_bytes());
        hasher.update(b"summary-v2 ");
        Ok(InitialPeerRecoveryPreview {
            peer_node_id: peer_node_id.to_owned(),
            generation,
            receiver_watermark,
            previous_handoff: Some(previous_handoff),
            capacity_quota_bytes: capacity.quota_bytes(),
            capacity_used_bytes: capacity.used_bytes(),
            capacity_available_bytes: capacity.filesystem_available_bytes(),
            capacity_required_bytes,
            capacity_filesystem_required_bytes,
            capacity_quota_shortfall_bytes,
            capacity_filesystem_shortfall_bytes,
            fingerprint: hex::encode(hasher.finalize()),
        })
    }

    pub(crate) fn arm_initial_peer_recovery(
        &mut self,
        peer_node_id: &str,
        expected_fingerprint: &str,
    ) -> Result<InitialPeerRecoveryPreview, RepositoryRuntimeError> {
        if self
            .snapshot
            .initial_peer_backfills
            .get(peer_node_id)
            .is_some_and(|checkpoint| {
                checkpoint.recovery_generation > 0 && !checkpoint.recovery_generation_consumed
            })
        {
            return Err(RepositoryRuntimeError::Storage(
                "history recovery generation is already armed".to_owned(),
            ));
        }
        let preview = self.preview_initial_peer_recovery(peer_node_id)?;
        if preview.fingerprint != expected_fingerprint {
            return Err(RepositoryRuntimeError::Storage(
                "history recovery fingerprint changed".to_owned(),
            ));
        }
        let previous_snapshot = self.snapshot.clone();
        let checkpoint = self
            .snapshot
            .initial_peer_backfills
            .entry(peer_node_id.to_owned())
            .or_default();
        checkpoint.summary_complete = false;
        checkpoint.summary_requires_tiered_backfill = true;
        checkpoint.summary_cursor = None;
        checkpoint.summary_pending_segment_ids.clear();
        checkpoint.summary_pending_next_cursor = None;
        checkpoint.recovery_generation = preview.generation;
        checkpoint.recovery_generation_consumed = false;
        // The retained anchor range is only known from the next signed repair response. Bind
        // this generation to the current source/epoch/stream and the next missing sequence;
        // the response may extend the missing tail up to its retained anchor exactly once.
        checkpoint.recovery_handoff = preview.previous_handoff.clone().map(|mut handoff| {
            let first_missing = preview
                .receiver_watermark
                .expect("recovery preview has a receiver watermark")
                .saturating_add(1);
            handoff.first_missing = first_missing;
            handoff.last_missing = first_missing.saturating_sub(1);
            handoff.next_sequence = first_missing;
            handoff.end_unix_seconds = 0;
            handoff
        });
        if let Err(error) = self.persist_control_state() {
            self.snapshot = previous_snapshot;
            return Err(error);
        }
        Ok(preview)
    }
}
