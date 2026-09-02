use crate::cluster_metadata::write_atomic_private;
use crate::ops::cli::ExitError;
use crate::resource_monitoring::CONTAINER_RUNTIME_IDENTITIES_FILE;
use std::fs;
use std::path::Path;
use tokio::process::Child;

pub(super) fn write_runtime_identities(
    data_dir: &Path,
    xray: &Child,
    cloudflared: Option<&Child>,
) -> Result<(), ExitError> {
    let xray = runtime_identity(xray).ok_or_else(|| {
        ExitError::new(6, "container_start_failed: resolve xray runtime identity")
    })?;
    let identities = ContainerRuntimeIdentities {
        xray,
        cloudflared: cloudflared.and_then(runtime_identity),
    };
    let raw = serde_json::to_vec(&identities).map_err(|error| {
        ExitError::new(
            6,
            format!("container_start_failed: serialize runtime identities: {error}"),
        )
    })?;
    write_atomic_private(&data_dir.join(CONTAINER_RUNTIME_IDENTITIES_FILE), &raw).map_err(|error| {
        ExitError::new(
            6,
            format!("container_start_failed: write runtime identities: {error}"),
        )
    })
}

fn runtime_identity(child: &Child) -> Option<ContainerRuntimeIdentity> {
    let pid = child.id()?;
    let start_time_ticks = process_start_time_ticks(pid)?;
    Some(ContainerRuntimeIdentity {
        pid,
        start_time_ticks,
    })
}

#[derive(serde::Serialize)]
struct ContainerRuntimeIdentities {
    xray: ContainerRuntimeIdentity,
    cloudflared: Option<ContainerRuntimeIdentity>,
}

#[derive(serde::Serialize)]
struct ContainerRuntimeIdentity {
    pid: u32,
    start_time_ticks: u64,
}

fn process_start_time_ticks(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    process_start_time_ticks_from_stat(&stat)
}

fn process_start_time_ticks_from_stat(stat: &str) -> Option<u64> {
    stat.rsplit_once(") ")?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::process_start_time_ticks_from_stat;

    #[test]
    fn parses_start_time_after_parenthesized_command() {
        let stat = "42 (xray worker) S 1 1 1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 12345 0";
        assert_eq!(process_start_time_ticks_from_stat(stat), Some(12_345));
    }
}
