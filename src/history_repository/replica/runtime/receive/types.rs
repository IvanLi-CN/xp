use super::super::*;

#[derive(Default)]
pub(crate) struct PendingRepositoryMutation {
    pub(crate) target_repository_id: String,
    pub(crate) progress_at_unix_seconds: Option<u64>,
    pub(crate) records: Vec<RepositoryHistoryRecordRow>,
    pub(crate) tombstones: Vec<RepositoryHistoryTombstone>,
    pub(crate) segments: Vec<RepositoryHistorySegmentRow>,
    pub(crate) applied_canonical_segments: Vec<(String, u64)>,
}

impl PendingRepositoryMutation {
    pub(super) fn into_storage_mutation(self) -> RepositoryReplicaMutation {
        RepositoryReplicaMutation {
            target_repository_id: self.target_repository_id,
            progress_at_unix_seconds: self.progress_at_unix_seconds,
            records: self.records,
            tombstones: self.tombstones,
            segments: self.segments,
            applied_canonical_segments: self.applied_canonical_segments,
        }
    }
}

pub(super) struct RepositoryWireRequest<'a> {
    pub(super) cluster_id: &'a str,
    pub(super) identity: &'a RepositoryNodeIdentity,
    pub(super) wire: &'a [u8],
    pub(super) now_unix_seconds: u64,
    pub(super) ready_repositories: &'a [String],
    pub(super) local_repository_id: &'a str,
    pub(super) allow_retained_anchor: bool,
    pub(super) rollback_snapshot: Option<RepositoryReplicaSnapshot>,
    pub(super) discard_gap_evidence_on_error: bool,
    pub(super) allow_retained_sequence_gap: bool,
    pub(super) retained_anchor_checkpoint: Option<RetainedAnchorCheckpointUpdate>,
}
