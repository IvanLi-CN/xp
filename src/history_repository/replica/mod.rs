//! Bounded replica, repair, and repository-retention contracts.

#![allow(dead_code)]

mod anti_entropy;
mod rendezvous;
mod retention;

#[allow(unused_imports)]
pub(crate) use anti_entropy::*;
#[allow(unused_imports)]
pub(crate) use rendezvous::*;
#[allow(unused_imports)]
pub(crate) use retention::*;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReplicaError {
    InvalidIdentifier,
    EmptyRepositories,
    DuplicateRepository,
    RepositoryBacklogFull,
    RecordTooLarge,
    InvalidRange,
    RepairLimitExceeded,
    StreamBacklogFull,
    TombstoneBacklogFull,
    TombstoneMissing,
    EpochNotAdvanced { minimum_epoch: u64 },
    ForkQuarantined { next_epoch: u64 },
    UnknownSchemaBacklogFull,
}

impl std::fmt::Display for ReplicaError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "history repository replica error: {self:?}")
    }
}

impl std::error::Error for ReplicaError {}
