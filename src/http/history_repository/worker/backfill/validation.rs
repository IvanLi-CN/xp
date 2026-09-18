use super::{HistoricalBackfillPageCursor, RepositoryInitialBackfillPage};
use crate::state::history_repository::{
    MAX_INITIAL_BACKFILL_PAGE_BYTES, MAX_INITIAL_BACKFILL_PAGE_RECORDS,
    replica::{tiered_backfill_record_bytes, validate_tiered_backfill_cursor},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct RepositoryInitialBackfillWirePage {
    records: Vec<Box<serde_json::value::RawValue>>,
    #[serde(default)]
    next_page_cursor: Option<String>,
}

pub(super) fn deserialize_initial_backfill_page(
    body: &[u8],
) -> anyhow::Result<RepositoryInitialBackfillPage> {
    let wire: RepositoryInitialBackfillWirePage = serde_json::from_slice(body)?;
    if wire.records.len() > MAX_INITIAL_BACKFILL_PAGE_RECORDS {
        anyhow::bail!("peer history backfill page exceeds record limit");
    }
    let records = wire
        .records
        .into_iter()
        .map(|raw| serde_json::from_str(raw.get()).map_err(anyhow::Error::from))
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(RepositoryInitialBackfillPage {
        records,
        next_page_cursor: wire.next_page_cursor,
    })
}

#[derive(Debug, Deserialize)]
struct TieredBackfillCursorPosition {
    observed_start_unix_seconds: u64,
    source_node_id: String,
    source_epoch: u64,
    stream: String,
    sequence: u64,
}

fn tiered_backfill_cursor_after(
    encoded: &str,
) -> anyhow::Result<Option<TieredBackfillCursorPosition>> {
    let bytes = URL_SAFE_NO_PAD.decode(encoded)?;
    let value = serde_json::from_slice::<serde_json::Value>(&bytes)?;
    let Some(after) = value.get("after").filter(|after| !after.is_null()) else {
        // Legacy compaction cursors are accepted for wire compatibility. The next response is
        // expected to upgrade them to the current export cursor, which carries an `after` key.
        return Ok(None);
    };
    serde_json::from_value(after.clone())
        .map(Some)
        .map_err(anyhow::Error::from)
}

fn historical_record_sort_key(
    record: &super::RepositoryInitialBackfillRecord,
) -> anyhow::Result<super::HistoricalBackfillSortKey> {
    if record.source_node_id.is_some()
        || record.source_epoch.is_some()
        || record.stream.is_some()
        || record.sequence.is_some()
    {
        anyhow::bail!("historical backfill record has a source cursor");
    }
    Ok(super::HistoricalBackfillSortKey {
        observed_at_unix_seconds: record.observed_at_unix_seconds,
        schema_id: record.schema_id.clone(),
        record_key: URL_SAFE_NO_PAD.decode(&record.record_key_base64)?,
    })
}

fn tiered_record_sort_key(
    record: &super::RepositoryInitialBackfillRecord,
) -> anyhow::Result<(u64, &str, u64, &str, u64)> {
    let (Some(source_node_id), Some(source_epoch), Some(stream), Some(sequence)) = (
        record.source_node_id.as_deref(),
        record.source_epoch,
        record.stream.as_deref(),
        record.sequence,
    ) else {
        anyhow::bail!("tiered backfill record has an incomplete source cursor");
    };
    Ok((
        record.observed_at_unix_seconds,
        source_node_id,
        source_epoch,
        stream,
        sequence,
    ))
}

fn validate_peer_backfill_cursor(
    next_encoded: &str,
    previous_encoded: Option<&str>,
    page: &super::RepositoryInitialBackfillPage,
    is_tiered_page: bool,
) -> anyhow::Result<()> {
    if let Ok(next_cursor) = HistoricalBackfillPageCursor::decode(next_encoded) {
        let previous_cursor = previous_encoded
            .map(HistoricalBackfillPageCursor::decode)
            .transpose()
            .map_err(|_| anyhow::anyhow!("peer history backfill cursor kind changed"))?;
        if let Some(previous_cursor) = previous_cursor.as_ref()
            && (next_cursor.after <= previous_cursor.after
                || (previous_cursor.snapshot_end_unix_seconds.is_some()
                    && next_cursor.snapshot_end_unix_seconds
                        != previous_cursor.snapshot_end_unix_seconds))
        {
            anyhow::bail!("peer history backfill page cursor did not advance");
        }
        if is_tiered_page || page.records.is_empty() {
            anyhow::bail!("peer history backfill cursor does not match page records");
        }
        let mut previous_record: Option<super::HistoricalBackfillSortKey> = None;
        for record in &page.records {
            let key = historical_record_sort_key(record)?;
            if previous_record
                .as_ref()
                .is_some_and(|previous| key <= previous.clone())
                || previous_cursor
                    .as_ref()
                    .is_some_and(|previous| key <= previous.after.clone())
            {
                anyhow::bail!("peer history backfill records are not strictly after cursor");
            }
            previous_record = Some(key);
        }
        if next_cursor.after != previous_record.expect("nonempty page") {
            anyhow::bail!("peer history backfill cursor does not match page tail");
        }
        return Ok(());
    }
    validate_tiered_backfill_cursor(next_encoded, previous_encoded)
        .map_err(|_| anyhow::anyhow!("peer history backfill cursor is invalid"))?;
    if !is_tiered_page {
        anyhow::bail!("tiered backfill cursor returned for historical page");
    }
    let Some(next_after) = tiered_backfill_cursor_after(next_encoded)? else {
        return Ok(());
    };
    let mut previous_record: Option<(u64, &str, u64, &str, u64)> = None;
    for record in &page.records {
        let key = tiered_record_sort_key(record)?;
        if previous_record
            .as_ref()
            .is_some_and(|previous| key <= *previous)
        {
            anyhow::bail!("peer tiered backfill records are not strictly ordered");
        }
        previous_record = Some(key);
    }
    if let Some(last_record) = previous_record {
        let next_key = (
            next_after.observed_start_unix_seconds,
            next_after.source_node_id.as_str(),
            next_after.source_epoch,
            next_after.stream.as_str(),
            next_after.sequence,
        );
        if next_key != last_record {
            anyhow::bail!("peer tiered backfill cursor does not match page tail");
        }
    }
    Ok(())
}

pub(super) fn validate_peer_backfill_page(
    page: &RepositoryInitialBackfillPage,
    previous_cursor_encoded: Option<&str>,
    cluster_id: &str,
) -> anyhow::Result<()> {
    if page.records.len() > MAX_INITIAL_BACKFILL_PAGE_RECORDS {
        anyhow::bail!("peer history backfill page exceeds record limit");
    }
    let records_are_tiered = page
        .records
        .first()
        .is_some_and(|record| record.source_node_id.is_some());
    if page
        .records
        .iter()
        .any(|record| record.source_node_id.is_some() != records_are_tiered)
    {
        anyhow::bail!("peer history backfill page mixes cursor formats");
    }
    let is_tiered_page = records_are_tiered
        || (page.records.is_empty()
            && page.next_page_cursor.as_deref().is_some_and(|cursor| {
                validate_tiered_backfill_cursor(cursor, previous_cursor_encoded).is_ok()
            }));
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
    validate_peer_backfill_cursor(
        next_page_cursor,
        previous_cursor_encoded,
        page,
        is_tiered_page,
    )
}
