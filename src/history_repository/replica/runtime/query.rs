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
        let mut remaining_skip = query.page_offset()?;
        for record in self
            .matching_records(query.subject_node_id())
            .filter(|record| {
                let (start, end) = retention::record_time_range(record);
                start <= query.range().end_unix_seconds()
                    && query.range().start_unix_seconds() <= end
            })
        {
            if remaining_skip > 0 {
                remaining_skip -= 1;
                continue;
            }
            let next_record: RepositoryHistoryRecord = record.clone().into();
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

    pub(super) fn matching_records(
        &self,
        subject_node_id: Option<&str>,
    ) -> impl Iterator<Item = &StoredRecord> {
        self.snapshot.records.iter().filter(move |record| {
            subject_node_id.is_none_or(|subject_node_id| record.subject_node_id == subject_node_id)
        })
    }
}
