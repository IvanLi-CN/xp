use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const EPOCH_FILE: &str = "internal-auth-v2.json";
const CUTOVER_MARKER_FILE: &str = "internal-auth-v2-cutover.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct AuthEpochRecord {
    epoch: u8,
}

fn mesh_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("mesh")
}

fn epoch_path(data_dir: &Path) -> PathBuf {
    mesh_dir(data_dir).join(EPOCH_FILE)
}

fn marker_path(data_dir: &Path) -> PathBuf {
    mesh_dir(data_dir).join(CUTOVER_MARKER_FILE)
}

pub fn is_v2_epoch(data_dir: &Path) -> bool {
    fs::read(epoch_path(data_dir))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<AuthEpochRecord>(&bytes).ok())
        .is_some_and(|record| record.epoch == 2)
}

/// Creates the single-use marker consumed by the first v2 binary during a maintenance window.
pub fn write_cutover_marker(data_dir: &Path) -> anyhow::Result<()> {
    let dir = mesh_dir(data_dir);
    fs::create_dir_all(&dir)?;
    write_atomic(
        &marker_path(data_dir),
        &serde_json::to_vec(&AuthEpochRecord { epoch: 2 })?,
    )
}

pub fn clear_cutover_marker(data_dir: &Path) {
    let _ = fs::remove_file(marker_path(data_dir));
}

/// Multi-member clusters cannot silently cross from the legacy auth contract. A marker is
/// consumed atomically into the durable epoch record once the new binary has started.
pub fn ensure_startup_epoch(data_dir: &Path, member_count: usize) -> anyhow::Result<()> {
    if is_v2_epoch(data_dir) {
        return Ok(());
    }
    if member_count <= 1 {
        fs::create_dir_all(mesh_dir(data_dir))?;
        return write_atomic(
            &epoch_path(data_dir),
            &serde_json::to_vec(&AuthEpochRecord { epoch: 2 })?,
        );
    }
    let marker = marker_path(data_dir);
    let bytes = fs::read(&marker).map_err(|_| {
        anyhow::anyhow!(concat!(
            "internal-auth v2 cutover marker is required for a multi-node cluster; ",
            "prepare the marker with the documented maintenance-window cutover procedure"
        ))
    })?;
    let record: AuthEpochRecord = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("internal-auth v2 cutover marker is invalid"))?;
    if record.epoch != 2 {
        anyhow::bail!("internal-auth v2 cutover marker has an unsupported epoch");
    }
    fs::create_dir_all(mesh_dir(data_dir))?;
    write_atomic(&epoch_path(data_dir), &serde_json::to_vec(&record)?)?;
    fs::remove_file(marker)?;
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let temp = path.with_extension("tmp");
    let mut file = fs::File::create(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(temp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_node_cutover_requires_and_consumes_marker() {
        let temp = tempfile::tempdir().expect("temp dir");
        assert!(ensure_startup_epoch(temp.path(), 2).is_err());
        write_cutover_marker(temp.path()).expect("marker");
        ensure_startup_epoch(temp.path(), 2).expect("consume marker");
        assert!(is_v2_epoch(temp.path()));
        assert!(!marker_path(temp.path()).exists());
    }

    #[test]
    fn single_node_startup_persists_the_v2_epoch() {
        let temp = tempfile::tempdir().expect("temp dir");
        ensure_startup_epoch(temp.path(), 1).expect("initialize single node epoch");
        assert!(is_v2_epoch(temp.path()));
        ensure_startup_epoch(temp.path(), 2).expect("persisted epoch avoids cutover gate");
    }
}
