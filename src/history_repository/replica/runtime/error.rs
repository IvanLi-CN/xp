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
    ClusterBindingMismatch,
    LegacySegmentCursorIndexPending,
    WriteStopped(HistoryWriteAvailability),
    StateLimitExceeded,
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
