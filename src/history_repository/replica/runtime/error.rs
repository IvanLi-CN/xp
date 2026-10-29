use crate::{
    history_sync::ProtocolError,
    state::history_repository::{control::HistoryWriteAvailability, query::QueryError},
};

use super::super::ReplicaError;

#[derive(Debug)]
pub(crate) enum RepositoryRuntimeError {
    Protocol(ProtocolError),
    Replica(ReplicaError),
    Query(QueryError),
    Storage(String),
    RepairResponseChanged,
    ClusterBindingMismatch,
    LegacySegmentCursorIndexPending,
    WriteStopped(HistoryWriteAvailability),
    StateLimitExceeded,
}

pub(super) fn backfill_target_stream(
    tombstone_stream: &str,
    target_stream: &str,
) -> Result<String, RepositoryRuntimeError> {
    let Some(source_node_id) = tombstone_stream.strip_prefix("tombstone-backfill-") else {
        return Ok(target_stream.to_owned());
    };
    (!source_node_id.is_empty())
        .then(|| format!("{target_stream}-backfill-{source_node_id}"))
        .ok_or_else(|| {
            RepositoryRuntimeError::Storage(
                "backfill tombstone stream has no source node identity".to_owned(),
            )
        })
}

impl std::fmt::Display for RepositoryRuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "history repository runtime error: {self:?}")
    }
}

impl std::error::Error for RepositoryRuntimeError {}

impl From<ProtocolError> for RepositoryRuntimeError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}

impl From<ReplicaError> for RepositoryRuntimeError {
    fn from(value: ReplicaError) -> Self {
        Self::Replica(value)
    }
}

impl From<QueryError> for RepositoryRuntimeError {
    fn from(value: QueryError) -> Self {
        Self::Query(value)
    }
}
