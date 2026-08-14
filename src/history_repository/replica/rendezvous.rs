use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest as _, Sha256};

use super::ReplicaError;

const PRIMARY_FAILURE_CYCLES_BEFORE_FAILOVER: u8 = 3;
const MAX_REPOSITORIES: usize = 4_096;
const MAX_TRACKED_SOURCES: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CollectorAssignment {
    primary: String,
    standby: Option<String>,
}

impl CollectorAssignment {
    pub(crate) fn primary(&self) -> &str {
        &self.primary
    }

    pub(crate) fn standby(&self) -> Option<&str> {
        self.standby.as_deref()
    }
}

/// Picks source collectors with a stable hash score for each source/repository pair.
pub(crate) fn rendezvous_collectors(
    source_node_id: &str,
    repositories: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<CollectorAssignment, ReplicaError> {
    validate_identifier(source_node_id)?;
    let mut unique = BTreeSet::new();
    for repository in repositories {
        let repository = repository.as_ref();
        validate_identifier(repository)?;
        if !unique.contains(repository) && unique.len() == MAX_REPOSITORIES {
            return Err(ReplicaError::RepositoryBacklogFull);
        }
        if !unique.insert(repository.to_owned()) {
            return Err(ReplicaError::DuplicateRepository);
        }
    }
    if unique.is_empty() {
        return Err(ReplicaError::EmptyRepositories);
    }

    let mut ranked: Vec<_> = unique
        .into_iter()
        .map(|repository| (rendezvous_score(source_node_id, &repository), repository))
        .collect();
    ranked.sort_by(|left, right| right.cmp(left));
    let mut ranked = ranked.into_iter().map(|(_, repository)| repository);
    let primary = ranked.next().expect("checked non-empty repositories");
    Ok(CollectorAssignment {
        primary,
        standby: ranked.next(),
    })
}

#[derive(Debug, Default)]
pub(crate) struct CollectorSelector {
    failed_primary_cycles: BTreeMap<String, u8>,
}

impl CollectorSelector {
    pub(crate) fn from_failure_cycles(failed_primary_cycles: BTreeMap<String, u8>) -> Self {
        Self {
            failed_primary_cycles,
        }
    }

    pub(crate) fn failure_cycles(&self) -> &BTreeMap<String, u8> {
        &self.failed_primary_cycles
    }

    pub(crate) fn select<'a>(
        &self,
        source_node_id: &str,
        assignment: &'a CollectorAssignment,
    ) -> Result<&'a str, ReplicaError> {
        validate_identifier(source_node_id)?;
        if assignment.standby.is_some()
            && self
                .failed_primary_cycles
                .get(source_node_id)
                .copied()
                .unwrap_or_default()
                >= PRIMARY_FAILURE_CYCLES_BEFORE_FAILOVER
        {
            return Ok(assignment.standby().expect("checked standby"));
        }
        Ok(assignment.primary())
    }

    pub(crate) fn record_primary_cycle(
        &mut self,
        source_node_id: &str,
        succeeded: bool,
    ) -> Result<(), ReplicaError> {
        validate_identifier(source_node_id)?;
        if succeeded {
            self.failed_primary_cycles.remove(source_node_id);
        } else {
            if !self.failed_primary_cycles.contains_key(source_node_id)
                && self.failed_primary_cycles.len() == MAX_TRACKED_SOURCES
            {
                return Err(ReplicaError::RepositoryBacklogFull);
            }
            let cycles = self
                .failed_primary_cycles
                .entry(source_node_id.to_owned())
                .or_default();
            *cycles = cycles.saturating_add(1);
        }
        Ok(())
    }
}

fn rendezvous_score(source_node_id: &str, repository_id: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"xp-history-repository-rendezvous-v1\0");
    hasher.update(source_node_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(repository_id.as_bytes());
    hasher.finalize().into()
}

fn validate_identifier(value: &str) -> Result<(), ReplicaError> {
    if value.trim().is_empty() || value != value.trim() || value.chars().any(char::is_control) {
        return Err(ReplicaError::InvalidIdentifier);
    }
    Ok(())
}
