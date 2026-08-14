use crate::{
    history_sync::{Acceptance, Cursor, SchemaCatalog, SyncRecord},
    state::history_repository::{
        control::HistoryWriteAvailability,
        query::{QueryPlan, StreamWatermark},
    },
};

use super::{
    KNOWN_SCHEMAS, RepositoryGap, RepositoryHistoryQueryResponse, RepositoryRuntimeError,
    RepositorySyncReceipt, RepositoryTombstoneAcknowledgement, RepositoryWatermark,
};

pub(super) fn known_schemas() -> SchemaCatalog {
    SchemaCatalog::new(
        KNOWN_SCHEMAS
            .iter()
            .map(|(schema, version)| ((*schema).to_owned(), *version)),
    )
}

pub(super) fn is_known_schema(record: &SyncRecord) -> bool {
    KNOWN_SCHEMAS.contains(&record.schema())
}

pub(super) fn watermark_from_cursor(
    cursor: Cursor,
) -> Result<StreamWatermark, RepositoryRuntimeError> {
    Ok(StreamWatermark::new(
        cursor.source_node_id(),
        cursor.source_epoch(),
        cursor.stream(),
        cursor.sequence(),
    )?)
}

pub(super) fn sync_receipt(
    acceptance: Acceptance,
    history_write_availability: HistoryWriteAvailability,
    tombstone_acknowledgements: Vec<RepositoryTombstoneAcknowledgement>,
) -> RepositorySyncReceipt {
    let acknowledgement =
        repository_watermark_from_cursor(acceptance.acknowledgement().watermark());
    let gap = acceptance.gap().map(|gap| RepositoryGap {
        requested: repository_watermark_from_cursor(gap.requested()),
        earliest_available: repository_watermark_from_cursor(gap.earliest_available()),
    });
    RepositorySyncReceipt {
        acknowledgement,
        gap,
        unknown_schema_records: acceptance.unknown_schema_records(),
        history_write_availability,
        tombstone_acknowledgements,
    }
}

fn repository_watermark_from_cursor(cursor: &Cursor) -> RepositoryWatermark {
    RepositoryWatermark {
        source_node_id: cursor.source_node_id().to_owned(),
        source_epoch: cursor.source_epoch(),
        stream: cursor.stream().to_owned(),
        sequence: cursor.sequence(),
    }
}

pub(super) fn serialized_response_overhead(
    plan: &QueryPlan,
) -> Result<usize, RepositoryRuntimeError> {
    let response = RepositoryHistoryQueryResponse {
        plan: plan.clone(),
        records: Vec::new(),
        records_truncated: true,
        next_page_cursor: Some(usize::MAX.to_string()),
    };
    serde_json::to_vec(&response)
        .map(|bytes| bytes.len())
        .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))
}
