use super::ContainerSpec;
use crate::cluster_metadata::ClusterPaths;
use crate::ops::cli::ExitError;
use crate::ops::paths::Paths;
use crate::ops::util::{chmod, write_string_if_changed};

pub(super) fn reconcile_configured_admin_token_hash(
    paths: &Paths,
    spec: &ContainerSpec,
) -> Result<(), ExitError> {
    if spec.startup.needs_join() {
        return Ok(());
    }
    let Some(hash) = spec.configured_admin_token_hash.as_deref() else {
        return Ok(());
    };
    let data_dir = paths.map_abs(&spec.data_dir);
    let target = ClusterPaths::new(&data_dir).admin_token_hash;
    write_string_if_changed(&target, &(hash.to_string() + "\n"))
        .map_err(|e| ExitError::new(4, format!("filesystem_error: sync admin token hash: {e}")))?;
    chmod(&target, 0o600).ok();
    Ok(())
}
