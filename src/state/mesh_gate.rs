use super::{DesiredStateApplyResult, PersistedState, StoreError};

pub(crate) fn default_mesh_enabled() -> bool {
    true
}

pub(crate) fn apply_mesh_enabled(
    state: &mut PersistedState,
    enabled: &bool,
) -> Result<DesiredStateApplyResult, StoreError> {
    state.mesh_enabled = *enabled;
    Ok(DesiredStateApplyResult::Applied)
}
