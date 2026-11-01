use super::*;
use sha2::{Digest as _, Sha256};

impl RepositoryReplicaRuntime {
    pub(crate) fn replication_summary_after_for_source(
        &self,
        after_segment_id: Option<&str>,
        deep_verification: bool,
        source_node_id: Option<&str>,
    ) -> Result<RepositoryReplicaSummary, RepositoryRuntimeError> {
        self.replication_summary_after_for_source_with_revision(
            after_segment_id,
            deep_verification,
            source_node_id,
            None,
        )
    }

    pub(crate) fn replication_summary_after_for_source_with_revision(
        &self,
        after_segment_id: Option<&str>,
        deep_verification: bool,
        source_node_id: Option<&str>,
        expected_source_summary_revision: Option<&str>,
    ) -> Result<RepositoryReplicaSummary, RepositoryRuntimeError> {
        if source_node_id.is_none()
            && after_segment_id.is_some_and(|cursor| cursor.starts_with("s:"))
        {
            return Err(RepositoryRuntimeError::Protocol(
                crate::history_sync::ProtocolError::CursorIdentityMismatch,
            ));
        }
        self.require_legacy_segment_cursor_index()?;
        let page_limit = super::super::REPLICATION_SEGMENT_PAGE_SIZE.saturating_add(1);
        let initial_source_summary_revision = source_node_id
            .map(|source| self.source_summary_revision(source))
            .transpose()?;
        let source_summary_changed = match (
            initial_source_summary_revision.as_deref(),
            expected_source_summary_revision,
        ) {
            (Some(current), Some(expected)) => current != expected,
            (None, Some(_)) => true,
            _ => false,
        };
        let mut page_after = if source_summary_changed {
            None
        } else {
            after_segment_id
        };
        let load_page =
            |after: Option<&str>| -> Result<Vec<SummarySegment>, RepositoryRuntimeError> {
                if self.uses_sqlite_history() {
                    Ok(self
                        .stored_segment_metadata_page_for_source(after, page_limit, source_node_id)?
                        .into_iter()
                        .map(|segment| SummarySegment {
                            cursor: if source_node_id.is_some() {
                                super::source_scoped_summary_segment_cursor(
                                    segment.contains_tombstone,
                                    &segment.id,
                                )
                            } else {
                                super::summary_segment_cursor(
                                    segment.contains_tombstone,
                                    &segment.id,
                                )
                            },
                            id: segment.id,
                        })
                        .collect())
                } else {
                    Ok(self
                        .stored_segments_page_for_source(after, page_limit, source_node_id)?
                        .into_iter()
                        .map(|segment| SummarySegment {
                            cursor: if source_node_id.is_some() {
                                super::source_scoped_segment_sync_cursor(&segment)
                            } else {
                                super::segment_sync_cursor(&segment)
                            },
                            id: segment.id,
                        })
                        .collect())
                }
            };
        let mut segments = load_page(page_after)?;
        let mut source_summary_revision = source_node_id
            .map(|source| self.source_summary_revision(source))
            .transpose()?;
        if source_node_id.is_some() && initial_source_summary_revision != source_summary_revision {
            // A write crossed the page read. Discard that page and retry from the source's
            // tombstone-first boundary so a new tombstone cannot be hidden behind an ordinary
            // cursor.
            page_after = None;
            segments = load_page(page_after)?;
            source_summary_revision = source_node_id
                .map(|source| self.source_summary_revision(source))
                .transpose()?;
        }
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
            && page_after.is_none()
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
                && page_after.is_none()
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
                && page_after.is_none()
                && (sequence_blocks_complete || self.partition_summaries_ready()),
            gaps: self.snapshot.gaps.iter().map(gap_summary).collect(),
            last_verified_unix_seconds: self.snapshot.last_verified_unix_seconds,
            history_truncated: self.snapshot.history_truncated,
            next_segment_id,
            source_summary_revision,
        })
    }

    fn source_summary_revision(
        &self,
        source_node_id: &str,
    ) -> Result<String, RepositoryRuntimeError> {
        if self.uses_sqlite_history() {
            return self
                .storage
                .repository_history_segment_revision()
                .map(|revision| revision.to_string())
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()));
        }
        let mut digest = Sha256::new();
        for segment in self
            .snapshot
            .segments
            .iter()
            .filter(|segment| segment.identity.node_id().as_str() == source_node_id)
        {
            digest.update(segment.id.as_bytes());
            digest.update([0]);
            digest.update(&segment.wire);
            digest.update([0xff]);
        }
        Ok(hex::encode(digest.finalize()))
    }
}
