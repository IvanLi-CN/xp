use super::*;

pub(super) fn push_backfill_record(
    records: &mut HistoricalBackfillCollector,
    schema_id: &str,
    node_id: &str,
    observed_at: &str,
    record_key: String,
    payload: serde_json::Value,
) -> anyhow::Result<()> {
    let observed_at = chrono::DateTime::parse_from_rfc3339(observed_at)
        .map_err(|error| {
            anyhow::anyhow!("parse persisted history timestamp {observed_at}: {error}")
        })?
        .timestamp()
        .try_into()
        .map_err(|_| anyhow::anyhow!("persisted history timestamp is before Unix epoch"))?;
    records.push((
        observed_at,
        source_record_with_key(
            schema_id,
            node_id,
            observed_at,
            record_key.into_bytes(),
            payload,
            false,
        )?,
    ))?;
    Ok(())
}
pub(crate) fn historical_record_batches(
    records: Vec<(u64, SyncRecord)>,
) -> anyhow::Result<Vec<(u64, Vec<SyncRecord>)>> {
    const MAX_RECORDS_PER_BACKFILL_SEGMENT: usize = 32;
    let mut batches = Vec::new();
    let mut current_timestamp = None;
    let mut current_stream = None;
    let mut current_records = Vec::new();
    for (observed_at, record) in records {
        let stream = if record.is_tombstone() {
            "tombstone"
        } else {
            source_stream_for_schema(record.schema().0)
                .ok_or_else(|| anyhow::anyhow!("unknown backfill schema"))?
        };
        let timestamp_changed = current_timestamp != Some(observed_at);
        let stream_changed = current_stream != Some(stream);
        let record_limit = current_records.len() == MAX_RECORDS_PER_BACKFILL_SEGMENT;
        let exceeds_byte_limit = if current_records.is_empty()
            || (!timestamp_changed && !stream_changed && !record_limit)
        {
            let mut candidate = current_records.clone();
            candidate.push(record.clone());
            backfill_segment_canonical_size(observed_at, stream, &candidate)?
                > MAX_INITIAL_BACKFILL_PAGE_BYTES
        } else {
            false
        };
        if timestamp_changed || stream_changed || record_limit || exceeds_byte_limit {
            if current_records.is_empty() {
                if exceeds_byte_limit {
                    anyhow::bail!("initial history backfill segment exceeds byte budget");
                }
            } else {
                batches.push((
                    current_timestamp.expect("backfill batch timestamp"),
                    std::mem::take(&mut current_records),
                ));
            }
            current_timestamp = Some(observed_at);
            current_stream = Some(stream);
        }
        current_records.push(record);
    }
    if let Some(timestamp) = current_timestamp {
        batches.push((timestamp, current_records));
    }
    Ok(batches)
}

pub(crate) fn queued_history_backfill_batches(
    batches: Vec<(u64, Vec<SyncRecord>)>,
) -> Vec<(Vec<SyncRecord>, u64)> {
    batches
        .into_iter()
        .map(|(observed_at_unix_seconds, records)| (records, observed_at_unix_seconds))
        .collect()
}

fn backfill_segment_canonical_size(
    timestamp: u64,
    stream: &str,
    records: &[SyncRecord],
) -> anyhow::Result<usize> {
    let result = CanonicalSegment::new(
        "backfill",
        Cursor::new("backfill", 0, stream, 0)?,
        records.to_vec(),
        None,
        timestamp,
        timestamp,
    )
    .and_then(|segment| segment.canonical_bytes());
    match result {
        Ok(bytes) => Ok(bytes.len()),
        Err(ProtocolError::SegmentCanonicalLimit { actual }) => Ok(actual),
        Err(error) => Err(error.into()),
    }
}
