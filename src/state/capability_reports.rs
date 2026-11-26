//! Persistence glue for `CompatNoop` notes, including signed node capability reports.
//!
//! Keeping this decoding here leaves the large `state` module to route a command with one line
//! per note kind. Both note kinds are additive payloads carried by a command that every older
//! voter already knows how to decode as a no-op.

use crate::capability_evidence::{NodeCapabilityReport, decode_capability_report_note};
use crate::state::probe_state::decode_node_egress_probe_compat_note;
use crate::state::{
    DesiredStateApplyResult, DesiredStateCommand, JsonSnapshotStore, PersistedState, StoreError,
};

/// Apply whatever a `CompatNoop` note carries.
///
/// A capability report for a node the cluster does not know is dropped: evidence must never
/// outlive its subject, otherwise a reused identity would inherit stale capabilities. An egress
/// probe note for an unknown node is likewise ignored so a deleted node cannot be resurrected into
/// the egress table.
pub(crate) fn apply_compat_noop(state: &mut PersistedState, note: &str) -> DesiredStateApplyResult {
    if let Some((node_id, probe)) = decode_node_egress_probe_compat_note(note) {
        if state.nodes.contains_key(&node_id) {
            state.node_egress_probes.insert(node_id, probe);
        }
        return DesiredStateApplyResult::Applied;
    }
    if let Some((node_id, report)) = decode_capability_report_note(note)
        && state.nodes.contains_key(&node_id)
    {
        state.node_capability_reports.insert(node_id, report);
    }
    DesiredStateApplyResult::Applied
}

pub(crate) fn remove_node_capability_report(state: &mut PersistedState, node_id: &str) {
    state.node_capability_reports.remove(node_id);
}

impl JsonSnapshotStore {
    pub fn get_node_capability_report(&self, node_id: &str) -> Option<NodeCapabilityReport> {
        self.state().node_capability_reports.get(node_id).cloned()
    }

    /// Apply a command the way a committed Raft write would, including local snapshot
    /// persistence. Tests use this to seed state that only Raft commands normally produce.
    pub fn apply_command_and_save(
        &mut self,
        command: DesiredStateCommand,
    ) -> Result<DesiredStateApplyResult, StoreError> {
        let result = command.apply(&mut self.state)?;
        self.save()?;
        Ok(result)
    }
}
