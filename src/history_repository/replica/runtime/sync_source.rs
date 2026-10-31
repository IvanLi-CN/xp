use super::*;

impl RepositoryReplicaRuntime {
    pub(crate) fn replication_summary_after_for_source(
        &self,
        after_segment_id: Option<&str>,
        deep_verification: bool,
        source_node_id: Option<&str>,
    ) -> Result<RepositoryReplicaSummary, RepositoryRuntimeError> {
        self.require_legacy_segment_cursor_index()?;
        let page_limit = super::super::REPLICATION_SEGMENT_PAGE_SIZE.saturating_add(1);
        let mut segments: Vec<SummarySegment> = if self.uses_sqlite_history() {
            self.stored_segment_metadata_page_for_source(
                after_segment_id,
                page_limit,
                source_node_id,
            )?
            .into_iter()
            .map(|segment| SummarySegment {
                cursor: summary_segment_cursor(segment.contains_tombstone, &segment.id),
                id: segment.id,
            })
            .collect()
        } else {
            self.stored_segments_page_for_source(after_segment_id, page_limit, source_node_id)?
                .into_iter()
                .map(|segment| SummarySegment {
                    cursor: segment_sync_cursor(&segment),
                    id: segment.id,
                })
                .collect()
        };
        let has_next_page = segments.len() > super::super::REPLICATION_SEGMENT_PAGE_SIZE;
        segments.truncate(super::super::REPLICATION_SEGMENT_PAGE_SIZE);
        let next_segment_id = if has_next_page {
            Some(
                segments
                    .last()
                    .expect("nonempty page had an extra segment")
                    .cursor
                    .clone(),
            )
        } else {
            None
        };
        let (sequence_blocks, sequence_blocks_complete) = if source_node_id.is_none()
            && deep_verification
            && after_segment_id.is_none()
            && self.uses_sqlite_history()
            && self.sequence_summary_blocks_ready()
        {
            self.sequence_summary_blocks()?
        } else {
            (Vec::new(), false)
        };
        Ok(RepositoryReplicaSummary {
            segment_ids: segments.into_iter().map(|segment| segment.id).collect(),
            partitions: if source_node_id.is_none()
                && deep_verification
                && after_segment_id.is_none()
                && (!self.uses_sqlite_history() || self.snapshot.partition_summaries_complete)
            {
                self.retained_partition_summaries()?
            } else {
                Vec::new()
            },
            summary_version: if sequence_blocks_complete { 3 } else { 1 },
            sequence_blocks,
            partitions_included: source_node_id.is_none()
                && deep_verification
                && after_segment_id.is_none()
                && (sequence_blocks_complete || self.partition_summaries_ready()),
            gaps: self.snapshot.gaps.iter().map(gap_summary).collect(),
            last_verified_unix_seconds: self.snapshot.last_verified_unix_seconds,
            history_truncated: self.snapshot.history_truncated,
            next_segment_id,
        })
    }
}
