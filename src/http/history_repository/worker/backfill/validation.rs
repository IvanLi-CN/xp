use super::{HistoricalBackfillPageCursor, RepositoryInitialBackfillPage};
use crate::state::history_repository::{
    MAX_INITIAL_BACKFILL_PAGE_BYTES, MAX_INITIAL_BACKFILL_PAGE_RECORDS,
    replica::{tiered_backfill_record_bytes, validate_tiered_backfill_cursor},
};

fn validate_peer_backfill_cursor(
    next_encoded: &str,
    previous_encoded: Option<&str>,
) -> anyhow::Result<()> {
    if let Ok(next_cursor) = HistoricalBackfillPageCursor::decode(next_encoded) {
        let Some(previous_encoded) = previous_encoded else {
            return Ok(());
        };
        let previous_cursor = HistoricalBackfillPageCursor::decode(previous_encoded)
            .map_err(|_| anyhow::anyhow!("peer history backfill cursor kind changed"))?;
        if next_cursor.after <= previous_cursor.after
            || (previous_cursor.snapshot_end_unix_seconds.is_some()
                && next_cursor.snapshot_end_unix_seconds
                    != previous_cursor.snapshot_end_unix_seconds)
        {
            anyhow::bail!("peer history backfill page cursor did not advance");
        }
        return Ok(());
    }
    validate_tiered_backfill_cursor(next_encoded, previous_encoded)
        .map_err(|_| anyhow::anyhow!("peer history backfill cursor is invalid"))
}

pub(super) fn validate_peer_backfill_page(
    page: &RepositoryInitialBackfillPage,
    previous_cursor_encoded: Option<&str>,
    cluster_id: &str,
) -> anyhow::Result<()> {
    if page.records.len() > MAX_INITIAL_BACKFILL_PAGE_RECORDS {
        anyhow::bail!("peer history backfill page exceeds record limit");
    }
    let is_tiered_page = page
        .records
        .first()
        .is_some_and(|record| record.source_node_id.is_some());
    if page
        .records
        .iter()
        .any(|record| record.source_node_id.is_some() != is_tiered_page)
    {
        anyhow::bail!("peer history backfill page mixes cursor formats");
    }
    let page_bytes = if is_tiered_page {
        page.records
            .iter()
            .enumerate()
            .try_fold(0_usize, |total, (index, record)| {
                let tiered = record
                    .clone()
                    .into_tiered_backfill_record()?
                    .ok_or_else(|| anyhow::anyhow!("tiered backfill record is incomplete"))?;
                let record_bytes = tiered_backfill_record_bytes(cluster_id, &tiered)
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                Ok::<_, anyhow::Error>(
                    total
                        .saturating_add(record_bytes)
                        .saturating_add(usize::from(index > 0)),
                )
            })?
    } else {
        serde_json::to_vec(&page.records)?.len()
    };
    if page_bytes > MAX_INITIAL_BACKFILL_PAGE_BYTES {
        anyhow::bail!("peer history backfill page exceeds byte limit");
    }
    let Some(next_page_cursor) = page.next_page_cursor.as_deref() else {
        return Ok(());
    };
    validate_peer_backfill_cursor(next_page_cursor, previous_cursor_encoded)
}
