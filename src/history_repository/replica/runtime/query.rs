use super::*;

impl RepositoryReplicaRuntime {
    pub(super) fn records_for(
        &self,
        query: &HistoryQuery,
        plan: &QueryPlan,
    ) -> Result<(Vec<RepositoryHistoryRecord>, bool, Option<String>), RepositoryRuntimeError> {
        let mut records = Vec::with_capacity(query.page_size());
        let mut response_bytes = serialized_response_overhead(plan)?;
        let mut truncated = false;
        let candidates = self.records_for_query(
            query.subject_node_id(),
            query.range().start_unix_seconds(),
            query.range().end_unix_seconds(),
            query.page_offset()?,
            query.page_size().saturating_add(1),
        )?;
        for record in candidates {
            let next_record: RepositoryHistoryRecord = record.into();
            let next_bytes = serde_json::to_vec(&next_record)
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
                .len()
                .saturating_add(usize::from(!records.is_empty()));
            if records.len() == query.page_size()
                || response_bytes.saturating_add(next_bytes) > MAX_QUERY_RESPONSE_BYTES
            {
                if records.is_empty() {
                    return Err(
                        crate::state::history_repository::query::QueryError::ResponseBudgetExceeded
                            .into(),
                    );
                }
                truncated = true;
                break;
            }
            response_bytes = response_bytes.saturating_add(next_bytes);
            records.push(next_record);
        }
        let next_page_cursor = truncated
            .then(|| query.next_page_cursor(records.len()))
            .transpose()?;
        Ok((records, truncated, next_page_cursor))
    }

    pub(super) fn records_for_query(
        &self,
        subject_node_id: Option<&str>,
        start_unix_seconds: u64,
        end_unix_seconds: u64,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<StoredRecord>, RepositoryRuntimeError> {
        if self.uses_sqlite_history() {
            return self.sqlite_records(
                subject_node_id,
                Some(start_unix_seconds),
                Some(end_unix_seconds),
                offset,
                limit,
            );
        }
        Ok(self
            .snapshot
            .records
            .iter()
            .filter(|record| {
                subject_node_id
                    .is_none_or(|subject_node_id| record.subject_node_id == subject_node_id)
            })
            .filter(|record| {
                let (start, end) = retention::record_time_range(record);
                start <= end_unix_seconds && start_unix_seconds <= end
            })
            .skip(offset)
            .take(limit)
            .cloned()
            .collect())
    }
}
