// These contracts are intentionally introduced before their Raft action consumers arrive.
#![allow(dead_code)]

mod capacity;
mod lifecycle;

#[allow(unused_imports)]
pub(crate) use capacity::{
    DEFAULT_HISTORY_REPOSITORY_QUOTA_BYTES, HISTORY_REPOSITORY_LOW_SPACE_GUARD_BYTES,
    HistoryWriteAvailability, RepositoryCapacity,
};
#[allow(unused_imports)]
pub(crate) use lifecycle::{
    RepositoryLifecycle, RepositoryMember, RepositoryMembership, RetirementDecision,
    apply_repository_membership,
};

#[cfg(test)]
mod tests;
