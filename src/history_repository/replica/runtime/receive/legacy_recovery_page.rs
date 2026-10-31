use super::*;

#[derive(Deserialize)]
struct SourceGaps {
    #[serde(default)]
    backpressure_gaps: BTreeMap<String, serde::de::IgnoredAny>,
}

struct BoundedSource(Vec<u8>);

impl std::io::Write for BoundedSource {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_RUNTIME_STATE_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other(
                "legacy source snapshot exceeds bound",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn has_tombstone_backpressure_gap(
    source: &LocalSourceState,
) -> Result<bool, RepositoryRuntimeError> {
    let mut bytes = BoundedSource(Vec::new());
    serde_json::to_writer(&mut bytes, source).map_err(storage_error)?;
    let gaps: SourceGaps = serde_json::from_slice(&bytes.0).map_err(storage_error)?;
    Ok(gaps
        .backpressure_gaps
        .keys()
        .any(|key| key.split('\0').next() == Some("tombstone")))
}

pub(super) fn validate_tombstone_wires(
    runtime: &RepositoryReplicaRuntime,
    originals: &[SourceDeliveryJournalRow],
) -> Result<(), RepositoryRuntimeError> {
    let tombstones = originals
        .iter()
        .filter(|row| row.stream == "tombstone")
        .collect::<Vec<_>>();
    if !tombstones.is_empty()
        && !runtime
            .storage
            .legacy_recovery_staged_contains_rows(&tombstones)
            .map_err(storage_error)?
    {
        return Err(failure(
            "legacy page tombstone is not in the verified source chain",
        ));
    }
    Ok(())
}
