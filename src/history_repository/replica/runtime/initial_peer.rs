use super::{RepositoryReplicaRuntime, RepositoryRuntimeError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub(crate) struct RetainedAnchorCheckpointUpdate {
    pub(crate) peer_node_id: String,
    pub(crate) response_id: String,
    pub(crate) response_complete: bool,
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
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct InitialPeerRetainedAnchorStream {
    pub(crate) source_node_id: String,
    pub(crate) source_epoch: u64,
    pub(crate) stream: String,
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
            checkpoint.retained_anchor_repair_response_seen = true;
            checkpoint.retained_anchor_repair_response_id = None;
        } else if !checkpoint.retained_anchor_repair_response_seen {
            checkpoint.retained_anchor_repair_response_id = retained_anchor_repair_response_id;
        }
        checkpoint.retained_anchor_streams = retained_anchor_streams;
        self.persist_control_state()
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
                ..InitialPeerBackfillCheckpoint::default()
            },
        );
        self.persist_control_state()
    }
}
