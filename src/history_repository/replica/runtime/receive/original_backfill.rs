use super::*;

#[path = "legacy_recovery.rs"]
mod legacy_recovery;
pub(crate) fn tombstone_acknowledgements_for_segment(
    segment: &crate::history_sync::CanonicalSegment,
    repository_id: &str,
) -> Result<Vec<RepositoryTombstoneAcknowledgement>, RepositoryRuntimeError> {
    let mut acknowledgements = Vec::new();
    for (offset, record) in segment.records().iter().enumerate() {
        if !record.is_tombstone() || !is_known_schema(record) {
            continue;
        }
        let sequence = segment
            .first_cursor()
            .sequence()
            .checked_add(
                u64::try_from(offset).map_err(|_| RepositoryRuntimeError::StateLimitExceeded)?,
            )
            .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
        let (schema_id, schema_version) = record.schema();
        let cursor = ReplicaCursor::new(
            segment.first_cursor().source_node_id(),
            segment.first_cursor().source_epoch(),
            segment.first_cursor().stream(),
            sequence,
        )?;
        acknowledgements.push(RepositoryTombstoneAcknowledgement {
            key: ReplicaRecord::new(
                &cursor,
                record.subject_node_id(),
                record.observer_node_id(),
                schema_id,
                schema_version,
                record.record_key().to_vec(),
                record.payload_bytes().to_vec(),
            )?
            .key(),
            repository_id: repository_id.to_owned(),
        });
    }
    Ok(acknowledgements)
}
