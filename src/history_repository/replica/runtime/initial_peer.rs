use super::{RepositoryReplicaRuntime, RepositoryRuntimeError};
use crate::history_sync::Cursor;
use serde::{Deserialize, Serialize};
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
    #[serde(default)]
    pub(crate) summary_tiered_handoff: Option<InitialPeerTieredHandoff>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct InitialPeerRetainedAnchorStream {
    pub(crate) source_node_id: String,
    pub(crate) source_epoch: u64,
    pub(crate) stream: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InitialPeerTieredHandoff {
    pub(crate) source_node_id: String,
    pub(crate) source_epoch: u64,
    pub(crate) stream: String,
    pub(crate) first_missing: u64,
    pub(crate) last_missing: u64,
    pub(crate) next_sequence: u64,
    pub(crate) end_unix_seconds: u64,
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
                summary_tiered_handoff: checkpoint.summary_tiered_handoff,
            },
        );
        self.persist_control_state()
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
        self.persist_control_state()
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
        let checkpoint = self
            .snapshot
            .initial_peer_backfills
            .entry(peer_node_id.to_owned())
            .or_default();
        if !checkpoint.retained_anchor_repair_response_seen {
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
        }
        checkpoint.summary_cursor = summary_cursor;
        checkpoint.summary_pending_segment_ids = pending_segment_ids;
        checkpoint.summary_pending_next_cursor = pending_next_cursor;
        checkpoint.summary_complete = summary_complete;
        checkpoint.summary_requires_tiered_backfill = summary_requires_tiered_backfill;
        if retained_anchor_repair_response_complete {
            checkpoint.retained_anchor_repair_response_id = None;
        } else if !checkpoint.retained_anchor_repair_response_seen {
            checkpoint.retained_anchor_repair_response_id = retained_anchor_repair_response_id;
        }
        if retained_anchor_allowance_complete {
            checkpoint.retained_anchor_repair_response_seen = true;
            checkpoint.retained_anchor_repair_response_id = None;
        }
        checkpoint.retained_anchor_streams = retained_anchor_streams;
        self.persist_control_state()
    }

    pub(crate) fn start_initial_peer_tiered_handoff(
        &mut self,
        peer_node_id: &str,
        handoff: InitialPeerTieredHandoff,
    ) -> Result<(), RepositoryRuntimeError> {
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
                summary_tiered_handoff: Some(handoff),
            },
        );
        self.persist_control_state()
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
        let advanced = self
            .receiver
            .as_mut()
            .expect("receiver checked above")
            .advance_declared_sequence_gap(&next, handoff.first_missing, handoff.last_missing)?;
        if !advanced {
            self.restore(&previous_receiver, previous_snapshot)?;
            return Err(RepositoryRuntimeError::Protocol(
                crate::history_sync::ProtocolError::SequenceGap {
                    expected: handoff.first_missing,
                    actual: handoff.next_sequence,
                },
            ));
        }
        self.snapshot
            .initial_peer_backfills
            .get_mut(peer_node_id)
            .expect("handoff checkpoint checked above")
            .summary_tiered_handoff = None;
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
                summary_tiered_handoff: prior.summary_tiered_handoff,
                ..InitialPeerBackfillCheckpoint::default()
            },
        );
        self.persist_control_state()
    }
}
