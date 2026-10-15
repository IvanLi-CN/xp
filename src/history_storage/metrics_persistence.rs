use super::*;

pub(super) fn read_at(
    connection: &Connection,
    now_unix_seconds: Option<u64>,
) -> Result<RepositoryDeliveryMetrics> {
    let mut metrics = connection
        .query_row(
            "SELECT generated_segments, generated_bytes,
                    acknowledged_segments, acknowledged_bytes,
                    applied_segments, applied_bytes,
                    source_delivery_attempts, source_delivery_successes,
                    source_transport_errors, source_response_errors,
                    source_response_changed,
                    source_explicit_unavailable, source_storage_errors,
                    last_attempt_at, last_progress_at,
                    counter_epoch,
                    window_started_at, window_generated_segments,
                    window_generated_bytes, window_acknowledged_segments,
                    window_acknowledged_bytes, window_applied_segments,
                    window_applied_bytes,
                    completed_window_started_at, completed_window_ended_at,
                    completed_generated_segments, completed_generated_bytes,
                    completed_acknowledged_segments, completed_acknowledged_bytes,
                    completed_applied_segments, completed_applied_bytes,
                    byte_counter_unit,
                    peak_generated_segments, peak_generated_bytes,
                    completed_peak_generated_segments, completed_peak_generated_bytes
             FROM repository_delivery_metrics WHERE singleton = 1",
            [],
            |row| {
                let generated_segments = checked_u64(row.get::<_, i64>(0)?, 0)?;
                let generated_bytes = checked_u64(row.get::<_, i64>(1)?, 1)?;
                let acknowledged_segments = checked_u64(row.get::<_, i64>(2)?, 2)?;
                let acknowledged_bytes = checked_u64(row.get::<_, i64>(3)?, 3)?;
                let applied_segments = checked_u64(row.get::<_, i64>(4)?, 4)?;
                let applied_bytes = checked_u64(row.get::<_, i64>(5)?, 5)?;
                let window_started_at = row
                    .get::<_, Option<i64>>(16)?
                    .map(|value| checked_u64(value, 16))
                    .transpose()?;
                let window =
                    |start_column: usize,
                     end_column: usize,
                     elapsed_seconds: u64|
                     -> rusqlite::Result<Option<RepositoryDeliveryWindow>> {
                        let Some(started_at) = row
                            .get::<_, Option<i64>>(start_column)?
                            .map(|value| checked_u64(value, start_column))
                            .transpose()?
                        else {
                            return Ok(None);
                        };
                        let Some(ended_at) = row
                            .get::<_, Option<i64>>(end_column)?
                            .map(|value| checked_u64(value, end_column))
                            .transpose()?
                        else {
                            return Ok(None);
                        };
                        let values = [
                            row.get::<_, Option<i64>>(25)?,
                            row.get::<_, Option<i64>>(26)?,
                            row.get::<_, Option<i64>>(27)?,
                            row.get::<_, Option<i64>>(28)?,
                            row.get::<_, Option<i64>>(29)?,
                            row.get::<_, Option<i64>>(30)?,
                        ];
                        let values = values
                            .into_iter()
                            .enumerate()
                            .map(|(index, value)| {
                                value
                                    .map(|value| checked_u64(value, 25 + index))
                                    .transpose()
                                    .map(|value| value.unwrap_or(0))
                            })
                            .collect::<rusqlite::Result<Vec<_>>>()?;
                        let completed_peak_generated_segments = row
                            .get::<_, Option<i64>>(34)?
                            .map(|value| checked_u64(value, 34))
                            .transpose()?;
                        let persisted_peak_generated_segments = row
                            .get::<_, Option<i64>>(32)?
                            .map(|value| checked_u64(value, 32))
                            .transpose()?;
                        let peak_generated_segments = completed_peak_generated_segments
                            .or(persisted_peak_generated_segments)
                            .unwrap_or(values[0]);
                        let completed_peak_generated_bytes = row
                            .get::<_, Option<i64>>(35)?
                            .map(|value| checked_u64(value, 35))
                            .transpose()?;
                        let persisted_peak_generated_bytes = row
                            .get::<_, Option<i64>>(33)?
                            .map(|value| checked_u64(value, 33))
                            .transpose()?;
                        let peak_generated_bytes = completed_peak_generated_bytes
                            .or(persisted_peak_generated_bytes)
                            .unwrap_or(values[1]);
                        Ok(Some(RepositoryDeliveryWindow::from_deltas_with_peak(
                            started_at,
                            ended_at,
                            elapsed_seconds,
                            values[0],
                            values[1],
                            peak_generated_segments,
                            peak_generated_bytes,
                            values[2],
                            values[3],
                            values[4],
                            values[5],
                        )))
                    };
                let last_completed_window = window(23, 24, DELIVERY_RATE_WINDOW_SECONDS)?;
                let last_completed_window = match last_completed_window {
                    Some(window) => Some(window),
                    None => match (now_unix_seconds, window_started_at) {
                        (Some(now), Some(started_at))
                            if now >= started_at.saturating_add(DELIVERY_RATE_WINDOW_SECONDS) =>
                        {
                            let baselines = [
                                row.get::<_, Option<i64>>(17)?,
                                row.get::<_, Option<i64>>(18)?,
                                row.get::<_, Option<i64>>(19)?,
                                row.get::<_, Option<i64>>(20)?,
                                row.get::<_, Option<i64>>(21)?,
                                row.get::<_, Option<i64>>(22)?,
                            ];
                            let baselines = baselines
                                .into_iter()
                                .enumerate()
                                .map(|(index, value)| {
                                    value
                                        .map(|value| checked_u64(value, 17 + index))
                                        .transpose()
                                        .map(|value| value.unwrap_or(0))
                                })
                                .collect::<rusqlite::Result<Vec<_>>>()?;
                            let peak_generated_segments = row
                                .get::<_, i64>(32)
                                .and_then(|value| checked_u64(value, 32))?;
                            let peak_generated_bytes = row
                                .get::<_, i64>(33)
                                .and_then(|value| checked_u64(value, 33))?;
                            Some(RepositoryDeliveryWindow::from_deltas_with_peak(
                                started_at,
                                started_at.saturating_add(DELIVERY_RATE_WINDOW_SECONDS),
                                DELIVERY_RATE_WINDOW_SECONDS,
                                generated_segments.saturating_sub(baselines[0]),
                                generated_bytes.saturating_sub(baselines[1]),
                                peak_generated_segments
                                    .max(generated_segments.saturating_sub(baselines[0])),
                                peak_generated_bytes
                                    .max(generated_bytes.saturating_sub(baselines[1])),
                                acknowledged_segments.saturating_sub(baselines[2]),
                                acknowledged_bytes.saturating_sub(baselines[3]),
                                applied_segments.saturating_sub(baselines[4]),
                                applied_bytes.saturating_sub(baselines[5]),
                            ))
                        }
                        _ => None,
                    },
                };
                Ok(RepositoryDeliveryMetrics {
                    generated_segments,
                    generated_bytes,
                    peak_generated_segments: checked_u64(row.get::<_, i64>(32)?, 32)?,
                    peak_generated_bytes: checked_u64(row.get::<_, i64>(33)?, 33)?,
                    acknowledged_segments,
                    acknowledged_bytes,
                    applied_segments,
                    applied_bytes,
                    source_delivery_attempts: checked_u64(row.get::<_, i64>(6)?, 6)?,
                    source_delivery_successes: checked_u64(row.get::<_, i64>(7)?, 7)?,
                    source_transport_errors: checked_u64(row.get::<_, i64>(8)?, 8)?,
                    source_response_errors: checked_u64(row.get::<_, i64>(9)?, 9)?,
                    source_response_changed: checked_u64(row.get::<_, i64>(10)?, 10)?,
                    source_explicit_unavailable: checked_u64(row.get::<_, i64>(11)?, 11)?,
                    source_storage_errors: checked_u64(row.get::<_, i64>(12)?, 12)?,
                    last_attempt_at: row
                        .get::<_, Option<i64>>(13)?
                        .map(|value| checked_u64(value, 13))
                        .transpose()?,
                    last_progress_at: row
                        .get::<_, Option<i64>>(14)?
                        .map(|value| checked_u64(value, 14))
                        .transpose()?,
                    counter_epoch: checked_u64(row.get::<_, i64>(15)?, 15)?,
                    byte_counter_unit: row.get::<_, String>(31)?,
                    rate_window_seconds: DELIVERY_RATE_WINDOW_SECONDS,
                    last_completed_window,
                    delivery_scopes: Vec::new(),
                })
            },
        )
        .map_err(sqlite_error)?;
    metrics.delivery_scopes = scopes::read_scopes(connection, now_unix_seconds)?;
    Ok(metrics)
}

pub(super) fn checked_u64(value: i64, column: usize) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}
