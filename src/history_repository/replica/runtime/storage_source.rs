use super::*;

pub(super) fn segment_sync_cursor_parts(cursor: Option<&str>) -> (bool, &str, bool) {
    match cursor {
        Some(cursor) if cursor.starts_with("s:t:") || cursor.starts_with("s:r:") => {
            (cursor.starts_with("s:r:"), &cursor[4..], true)
        }
        Some(cursor) if cursor.starts_with("t:") || cursor.starts_with("r:") => {
            (cursor.starts_with("r:"), &cursor[2..], false)
        }
        Some(_) | None => (false, "", false),
    }
}

impl RepositoryReplicaRuntime {
    pub(crate) fn stored_segments_page_for_source(
        &self,
        after_id: Option<&str>,
        limit: usize,
        source_node_id: Option<&str>,
    ) -> Result<Vec<StoredSegment>, RepositoryRuntimeError> {
        if source_node_id.is_none() {
            return self.stored_segments_page(after_id, limit);
        }
        let source_node_id = source_node_id.expect("source was checked above");
        if !self.uses_sqlite_history() {
            let (cursor_tombstone_rank, after_id, cursor_is_source_scoped) =
                segment_sync_cursor_parts(after_id);
            let mut segments = self
                .snapshot
                .segments
                .iter()
                .filter(|segment| segment.identity.node_id().as_str() == source_node_id)
                .cloned()
                .map(|segment| {
                    (
                        segment_tombstone_rank(&segment),
                        segment_cursor_order(&segment),
                        segment,
                    )
                })
                .collect::<Vec<_>>();
            segments.sort_by(|(left_rank, left_order, _), (right_rank, right_order, _)| {
                (left_rank, left_order).cmp(&(right_rank, right_order))
            });
            let (after_tombstone_rank, after_order) = if cursor_is_source_scoped {
                match segments
                    .iter()
                    .find(|(_, _, segment)| segment.id == after_id)
                {
                    Some((rank, order, _)) if *rank == cursor_tombstone_rank => {
                        (cursor_tombstone_rank, Some(order.clone()))
                    }
                    _ => (false, None),
                }
            } else {
                (false, None)
            };
            let mut segments = segments
                .into_iter()
                .filter(|(rank, order, _)| {
                    after_order
                        .as_ref()
                        .map_or(*rank >= after_tombstone_rank, |after_order| {
                            (rank, order) > (&after_tombstone_rank, after_order)
                        })
                })
                .map(|(_, _, segment)| segment)
                .collect::<Vec<_>>();
            segments.truncate(limit);
            return Ok(segments);
        }
        self.storage
            .repository_history_segments_page_for_source(after_id, Some(source_node_id), limit)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
            .into_iter()
            .map(StoredSegment::from_sqlite_row)
            .collect::<Result<Vec<_>, _>>()
    }

    pub(crate) fn stored_segment_metadata_page_for_source(
        &self,
        after_id: Option<&str>,
        limit: usize,
        source_node_id: Option<&str>,
    ) -> Result<Vec<super::StoredSegmentMetadata>, RepositoryRuntimeError> {
        if source_node_id.is_none() {
            return self.stored_segment_metadata_page(after_id, limit);
        }
        self.storage
            .repository_history_segment_metadata_page_for_source(after_id, source_node_id, limit)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
            .into_iter()
            .map(|row| {
                Ok(super::StoredSegmentMetadata {
                    id: row.id,
                    contains_tombstone: row.contains_tombstone,
                })
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "storage_source_tests.rs"]
mod storage_source_tests;
