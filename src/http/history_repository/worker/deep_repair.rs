use crate::state::history_repository::replica::{
    ReplicaWork, RepositoryReplicaRuntime, RepositoryRuntimeError,
};

pub(super) fn deep_repair_requires_tiered_backfill(
    work: ReplicaWork,
    remaining_segment_repairs: bool,
    repair_remains_after_segment_repairs: bool,
) -> bool {
    work.is_deep_verification()
        && !remaining_segment_repairs
        && repair_remains_after_segment_repairs
}

pub(super) fn restart_tiered_backfill_after_incomplete_deep_repair(
    runtime: &mut RepositoryReplicaRuntime,
    peer_node_id: &str,
    work: ReplicaWork,
    remaining_segment_repairs: bool,
    repair_remains_after_segment_repairs: bool,
) -> Result<bool, RepositoryRuntimeError> {
    if !deep_repair_requires_tiered_backfill(
        work,
        remaining_segment_repairs,
        repair_remains_after_segment_repairs,
    ) {
        return Ok(false);
    }
    runtime.restart_initial_peer_backfill(peer_node_id)?;
    Ok(true)
}
