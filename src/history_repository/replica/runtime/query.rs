use super::*;

impl RepositoryReplicaRuntime {
    pub(crate) fn query(
        &self,
        repository_id: &str,
        query: HistoryQuery,
        local: LocalQueryMetadata,
    ) -> Result<RepositoryHistoryQueryResponse, RepositoryRuntimeError> {
        let local_coverage = QueryCoverage::new(
            QueryRange::new(
                local.observed_start_unix_seconds,
                local.observed_end_unix_seconds,
            )?,
            QueryRange::new(
                local.observed_start_unix_seconds,
                local.observed_end_unix_seconds,
            )?,
        );
        let local_gap = QueryGap::new(
            query.range().start_unix_seconds(),
            query.range().end_unix_seconds(),
            false,
        )?;
        let local_candidate = QueryCandidate::local(
            local_coverage,
            std::iter::empty(),
            [local_gap],
            local.clock_skew_seconds,
        )?;
        let mut candidates = vec![local_candidate];
        if let Some(coverage) = self.repository_coverage(query.subject_node_id())? {
            let watermarks = self
                .receiver
                .as_ref()
                .map(SegmentReceiver::continuous_watermarks)
                .unwrap_or_default()
                .into_iter()
                .map(watermark_from_cursor)
                .collect::<Result<Vec<_>, _>>()?;
            let mut gaps = self
                .all_prefix_aware_gaps()
                .filter(|gap| {
                    query
                        .subject_node_id()
                        .is_none_or(|subject_node_id| gap.source_node_id == subject_node_id)
                })
                .map(|gap| {
                    QueryGap::new_with_reason(
                        gap.start_unix_seconds,
                        gap.end_unix_seconds,
                        gap.permanent,
                        gap.reason.clone(),
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            if let Some((start, end)) = self.incomplete_aggregate_gap(&query)? {
                gaps.push(QueryGap::new(start, end, true)?);
            }
            if self.snapshot.history_truncated {
                gaps.push(QueryGap::new(
                    coverage.observed().start_unix_seconds(),
                    coverage.observed().end_unix_seconds(),
                    true,
                )?);
            }
            candidates.push(QueryCandidate::ready(
                repository_id,
                coverage,
                watermarks,
                gaps,
                0,
            )?);
        }
        let plan = QuerySelector::select(&query, candidates)?;
        let (records, records_truncated, next_page_cursor) = if plan.repository_id().is_some() {
            self.records_for(&query, &plan)?
        } else {
            (Vec::new(), false, None)
        };
        Ok(RepositoryHistoryQueryResponse {
            plan,
            records,
            records_truncated,
            next_page_cursor,
        })
    }

    pub(crate) fn query_local_only(
        &self,
        query: HistoryQuery,
        local: LocalQueryMetadata,
    ) -> Result<RepositoryHistoryQueryResponse, RepositoryRuntimeError> {
        let coverage = QueryCoverage::new(
            QueryRange::new(
                local.observed_start_unix_seconds,
                local.observed_end_unix_seconds,
            )?,
            QueryRange::new(
                local.observed_start_unix_seconds,
                local.observed_end_unix_seconds,
            )?,
        );
        let gap = QueryGap::new(
            query.range().start_unix_seconds(),
            query.range().end_unix_seconds(),
            false,
        )?;
        let plan = QuerySelector::select(
            &query,
            [QueryCandidate::local(
                coverage,
                std::iter::empty(),
                [gap],
                local.clock_skew_seconds,
            )?],
        )?;
        Ok(RepositoryHistoryQueryResponse {
            plan,
            records: Vec::new(),
            records_truncated: false,
            next_page_cursor: None,
        })
    }

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
            query.schema_id(),
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
        schema_id: Option<&str>,
        start_unix_seconds: u64,
        end_unix_seconds: u64,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<StoredRecord>, RepositoryRuntimeError> {
        if self.uses_sqlite_history() {
            return self.sqlite_records_for_schema(
                subject_node_id,
                schema_id,
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
            .filter(|record| schema_id.is_none_or(|schema_id| record.schema_id == schema_id))
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
