use super::{RepositoryReplicaRuntime, RepositoryRuntimeError};

pub(crate) const MAX_SEQUENCE_SUMMARY_BLOCKS: usize = 1024;

impl RepositoryReplicaRuntime {
    pub(crate) fn advance_sequence_summary_block_rebuild_page(
        &mut self,
    ) -> Result<bool, RepositoryRuntimeError> {
        if !self.uses_sqlite_history() || self.snapshot.sequence_summary_blocks_complete {
            return Ok(true);
        }
        if !self.snapshot.sequence_summary_migration_complete {
            let (blocks, cursor, complete) = self
                .storage
                .repository_history_sequence_summary_migration_page(
                    self.snapshot.sequence_summary_migration_cursor.as_ref(),
                    256,
                )
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
            self.storage
                .mark_repository_history_sequence_summary_blocks_dirty(&blocks)
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
            self.snapshot.sequence_summary_migration_cursor = cursor;
            self.snapshot.sequence_summary_migration_complete = complete;
            self.persist_control_state()?;
            return Ok(false);
        }
        let dirty = self
            .storage
            .repository_history_dirty_sequence_summary_blocks(4)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        if dirty.is_empty() {
            self.snapshot.sequence_summary_blocks_complete = true;
            self.persist_control_state()?;
            return Ok(true);
        }
        for (source_node_id, source_epoch, stream, block_index) in dirty {
            if let Err(error) = self
                .storage
                .rebuild_repository_history_sequence_summary_block(
                    &source_node_id,
                    source_epoch,
                    &stream,
                    block_index,
                )
            {
                tracing::warn!(
                    %error,
                    source_node_id,
                    source_epoch,
                    stream,
                    block_index,
                    "sequence summary block rebuild deferred"
                );
                return Ok(false);
            }
        }
        self.persist_control_state()?;
        Ok(false)
    }

    pub(crate) fn sequence_summary_blocks(
        &self,
    ) -> Result<(Vec<super::RepositorySequenceBlockSummary>, bool), RepositoryRuntimeError> {
        if !self.uses_sqlite_history() {
            return Ok((Vec::new(), true));
        }
        let (blocks, complete) = self
            .storage
            .repository_history_sequence_summary_blocks_bounded(MAX_SEQUENCE_SUMMARY_BLOCKS + 1)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        let blocks = blocks
            .into_iter()
            .take(MAX_SEQUENCE_SUMMARY_BLOCKS)
            .map(|block| super::RepositorySequenceBlockSummary {
                source_node_id: block.source_node_id,
                source_epoch: block.source_epoch,
                stream: block.stream,
                block_index: block.block_index,
                first_sequence: block.first_sequence,
                last_sequence: block.last_sequence,
                hash: block.digest,
                record_count: block.record_count,
            })
            .collect();
        Ok((blocks, complete))
    }
}
