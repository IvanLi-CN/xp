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

/// Returns whether the durable cluster epoch is v2. A missing record represents a legacy
/// cluster, but a record that cannot be read or decoded must never be treated as legacy.
pub fn is_v2_epoch(data_dir: &Path) -> anyhow::Result<bool> {
    let path = epoch_path(data_dir);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error).map_err(|error| {
                anyhow::anyhow!("read internal-auth epoch {}: {error}", path.display())
            });
        }
    };
    let record: AuthEpochRecord = serde_json::from_slice(&bytes).map_err(|error| {
        anyhow::anyhow!("decode internal-auth epoch {}: {error}", path.display())
    })?;
    match record.epoch {
        2 => Ok(true),
        epoch => anyhow::bail!(
            "internal-auth epoch {} has unsupported value {epoch}",
            path.display()
        ),
    }
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

pub fn clear_cutover_marker(data_dir: &Path) -> anyhow::Result<()> {
    let path = marker_path(data_dir);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).map_err(|error| {
            anyhow::anyhow!(
                "remove internal-auth cutover marker {}: {error}",
                path.display()
            )
        }),
    }
}

/// Multi-member clusters cannot silently cross from the legacy auth contract. A marker is
/// consumed atomically into the durable epoch record once the new binary has started.
pub fn ensure_startup_epoch(data_dir: &Path, member_count: usize) -> anyhow::Result<()> {
    if is_v2_epoch(data_dir)? {
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
        assert!(is_v2_epoch(temp.path()).expect("read epoch"));
        assert!(!marker_path(temp.path()).exists());
    }

    #[test]
    fn single_node_startup_persists_the_v2_epoch() {
        let temp = tempfile::tempdir().expect("temp dir");
        ensure_startup_epoch(temp.path(), 1).expect("initialize single node epoch");
        assert!(is_v2_epoch(temp.path()).expect("read epoch"));
        ensure_startup_epoch(temp.path(), 2).expect("persisted epoch avoids cutover gate");
    }

    #[test]
    fn malformed_epoch_does_not_fall_back_to_legacy() {
        let temp = tempfile::tempdir().expect("temp dir");
        fs::create_dir_all(mesh_dir(temp.path())).expect("mesh dir");
        fs::write(epoch_path(temp.path()), "not json").expect("epoch file");

        assert!(is_v2_epoch(temp.path()).is_err());
        assert!(ensure_startup_epoch(temp.path(), 2).is_err());
    }
}
