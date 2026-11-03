use super::*;
use std::collections::BTreeMap;

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
            let (after_tombstone_rank, after_order) = if cursor_is_source_scoped {
                match self
                    .snapshot
                    .segments
                    .iter()
                    .filter(|segment| segment.identity.node_id().as_str() == source_node_id)
                    .find(|segment| segment.id == after_id)
                    .map(|segment| {
                        (
                            segment_tombstone_rank(segment),
                            segment_cursor_order(segment),
                        )
                    }) {
                    Some((rank, order)) if rank == cursor_tombstone_rank => {
                        (cursor_tombstone_rank, Some(order))
                    }
                    _ => (false, None),
                }
            } else {
                // An unscoped legacy cursor is not a safe position inside this source. Restart
                // the source inventory from its tombstone-first bound, matching SQLite.
                (false, None)
            };
            if limit == 0 {
                return Ok(Vec::new());
            }
            let mut selected = BTreeMap::new();
            for segment in self
                .snapshot
                .segments
                .iter()
                .filter(|segment| segment.identity.node_id().as_str() == source_node_id)
            {
                let rank = segment_tombstone_rank(segment);
                let order = segment_cursor_order(segment);
                let is_after_cursor = after_order
                    .as_ref()
                    .map_or(rank >= after_tombstone_rank, |cursor| {
                        (rank, &order) > (after_tombstone_rank, cursor)
                    });
                if !is_after_cursor {
                    continue;
                }
                selected.insert((rank, order), segment);
                if selected.len() > limit {
                    let last = selected
                        .keys()
                        .next_back()
                        .cloned()
                        .expect("bounded snapshot page is nonempty");
                    selected.remove(&last);
                }
            }
            return Ok(selected.into_values().cloned().collect());
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
