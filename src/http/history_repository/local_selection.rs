//! Local-completeness shortcut for repository history queries.
//!
//! Peer fan-out exists so a request can still be answered after local retention dropped a range.
//! Asking anyway turns every panel poll into a cluster round trip, so the shortcut lives beside
//! the query module instead of inside it.

use super::*;

/// Whether this node's own copy already answers the request without contacting any peer.
pub(super) fn local_repository_covers(
    state: &AppState,
    local_response: &RepositoryHistoryQueryResponse,
    request: &RepositoryHistoryQuery,
    local_is_ready: bool,
) -> bool {
    if !local_is_ready {
        return false;
    }
    let plan = local_response.plan();
    let covered = plan.coverage().is_some_and(|coverage| {
        coverage.observed().start_unix_seconds() <= request.start_unix_seconds
            && coverage.observed().end_unix_seconds() >= request.end_unix_seconds
    });
    covered
        && plan.gaps().iter().all(|gap| {
            let range = gap.range();
            range.end_unix_seconds() < request.start_unix_seconds
                || range.start_unix_seconds() > request.end_unix_seconds
        })
        && plan.repository_id() == Some(&state.cluster.node_id)
}
