use std::collections::BTreeMap;

use super::*;

impl HistoryStorage {
    pub(crate) fn source_delivery_journal_has_unloaded_tail(
        &self,
        pending_by_stream: &BTreeMap<String, usize>,
    ) -> Result<bool> {
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Ok(false);
        };
        let mut statement = connection
            .prepare(
                "SELECT stream, COUNT(*)
                 FROM source_delivery_journal
                 GROUP BY stream",
            )
            .map_err(sqlite_error)?;
        let durable_by_stream = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(sqlite_error)?
            .collect::<std::result::Result<BTreeMap<_, _>, _>>()
            .map_err(sqlite_error)?;
        Ok(durable_by_stream
            .into_iter()
            .any(|(stream, durable_count)| {
                let loaded_count = pending_by_stream.get(&stream).copied().unwrap_or_default();
                i64::try_from(loaded_count)
                    .map(|loaded_count| durable_count > loaded_count)
                    .unwrap_or(true)
            }))
    }
}
