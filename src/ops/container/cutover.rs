use std::path::Path;

use super::*;

pub(super) fn write_marker_if(data_dir: &Path, should_write: bool) -> Result<(), ExitError> {
    if !should_write {
        return Ok(());
    }
    crate::internal_auth_epoch::write_cutover_marker(data_dir).map_err(|error| {
        ExitError::new(
            7,
            format!("service_error: write internal-auth v2 cutover marker: {error}"),
        )
    })
}
