use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use crate::state::history_repository::{
    control::capacity::{RepositoryCapacity, RepositoryCapacityError},
    identity::{RepositoryIdentityError, RepositoryNodeId, RepositoryNodeIdentity},
};

const READY_STABILITY_WINDOW_SECONDS: u64 = 5 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RepositoryControlError {
    EmptyMembership,
    DuplicateRepositoryMember {
        node_id: RepositoryNodeId,
    },
    MissingRepository {
        node_id: RepositoryNodeId,
    },
    InvalidLifecycleTransition {
        node_id: RepositoryNodeId,
        from: RepositoryLifecycle,
        action: &'static str,
    },
    CatchUpNotComplete {
        node_id: RepositoryNodeId,
    },
    StabilityWindowIncomplete {
        node_id: RepositoryNodeId,
        required_seconds: u64,
        elapsed_seconds: u64,
    },
    ClockBeforeCatchUp {
        node_id: RepositoryNodeId,
    },
    NoReadyConvergedReplacement {
        node_id: RepositoryNodeId,
    },
    LastRepositoryRequiresEmergencyDecision {
        node_id: RepositoryNodeId,
    },
    EmergencyReasonRequired,
    InvalidPersistedMember {
        node_id: RepositoryNodeId,
        reason: &'static str,
    },
    Identity(RepositoryIdentityError),
    Capacity(RepositoryCapacityError),
}

impl std::fmt::Display for RepositoryControlError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyMembership => formatter.write_str("repository membership must not be empty"),
            Self::DuplicateRepositoryMember { node_id } => {
                write!(
                    formatter,
                    "duplicate repository member: {}",
                    node_id.as_str()
                )
            }
            Self::MissingRepository { node_id } => {
                write!(
                    formatter,
                    "repository member not found: {}",
                    node_id.as_str()
                )
            }
            Self::InvalidLifecycleTransition {
                node_id,
                from,
                action,
            } => write!(
                formatter,
                "repository {} cannot {action} from lifecycle {from:?}",
                node_id.as_str()
            ),
            Self::CatchUpNotComplete { node_id } => {
                write!(
                    formatter,
                    "repository {} has not completed catch-up",
                    node_id.as_str()
                )
            }
            Self::StabilityWindowIncomplete {
                node_id,
                required_seconds,
                elapsed_seconds,
            } => write!(
                formatter,
                "repository {} stable {elapsed_seconds}s; {required_seconds}s required",
                node_id.as_str()
            ),
            Self::ClockBeforeCatchUp { node_id } => {
                write!(
                    formatter,
                    "ready time precedes catch-up for repository {}",
                    node_id.as_str()
                )
            }
            Self::NoReadyConvergedReplacement { node_id } => write!(
                formatter,
                "repository {} needs another ready converged repository before ordinary retirement",
                node_id.as_str()
            ),
            Self::LastRepositoryRequiresEmergencyDecision { node_id } => write!(
                formatter,
                "repository {} is the last active repository and requires an emergency decision",
                node_id.as_str()
            ),
            Self::EmergencyReasonRequired => {
                formatter.write_str("emergency retirement reason is required")
            }
            Self::InvalidPersistedMember { node_id, reason } => write!(
                formatter,
                "invalid persisted repository member {}: {reason}",
                node_id.as_str()
            ),
            Self::Identity(error) => error.fmt(formatter),
            Self::Capacity(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for RepositoryControlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Identity(error) => Some(error),
            Self::Capacity(error) => Some(error),
            _ => None,
        }
    }
}

impl From<RepositoryIdentityError> for RepositoryControlError {
    fn from(value: RepositoryIdentityError) -> Self {
        Self::Identity(value)
    }
}

impl From<RepositoryCapacityError> for RepositoryControlError {
    fn from(value: RepositoryCapacityError) -> Self {
        Self::Capacity(value)
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RepositoryLifecycle {
    #[default]
    Syncing,
    Ready,
    Retired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RetirementDecision {
    Ordinary,
    ForceEmergency { reason: String },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct RepositoryMember {
    identity: RepositoryNodeIdentity,
    #[serde(default)]
    lifecycle: RepositoryLifecycle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    catch_up_completed_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ready_at: Option<u64>,
    #[serde(default)]
    replica_converged: bool,
    #[serde(default)]
    capacity: RepositoryCapacity,
}

impl RepositoryMember {
    pub(crate) fn new(
        identity: RepositoryNodeIdentity,
        capacity: RepositoryCapacity,
    ) -> Result<Self, RepositoryControlError> {
        let member = Self {
            identity,
            lifecycle: RepositoryLifecycle::Syncing,
            catch_up_completed_at: None,
            ready_at: None,
            replica_converged: false,
            capacity,
        };
        member.validate()?;
        Ok(member)
    }

    pub(crate) fn node_id(&self) -> &RepositoryNodeId {
        self.identity.node_id()
    }

    pub(crate) fn identity(&self) -> &RepositoryNodeIdentity {
        &self.identity
    }

    pub(crate) fn lifecycle(&self) -> &RepositoryLifecycle {
        &self.lifecycle
    }

    pub(crate) fn catch_up_completed_at(&self) -> Option<u64> {
        self.catch_up_completed_at
    }

    pub(crate) fn ready_at(&self) -> Option<u64> {
        self.ready_at
    }

    pub(crate) fn replica_converged(&self) -> bool {
        self.replica_converged
    }

    pub(crate) fn capacity(&self) -> &RepositoryCapacity {
        &self.capacity
    }

    fn validate(&self) -> Result<(), RepositoryControlError> {
        self.capacity.validate()?;
        match self.lifecycle {
            RepositoryLifecycle::Syncing if self.replica_converged => {
                Err(RepositoryControlError::InvalidPersistedMember {
                    node_id: self.node_id().clone(),
                    reason: "syncing repository cannot be converged",
                })
            }
            RepositoryLifecycle::Ready if self.catch_up_completed_at.is_none() => {
                Err(RepositoryControlError::InvalidPersistedMember {
                    node_id: self.node_id().clone(),
                    reason: "ready repository requires completed catch-up",
                })
            }
            RepositoryLifecycle::Ready if !self.ready_at_is_stable() => {
                Err(RepositoryControlError::InvalidPersistedMember {
                    node_id: self.node_id().clone(),
                    reason: "ready repository requires five stable minutes after catch-up",
                })
            }
            RepositoryLifecycle::Retired if self.replica_converged => {
                Err(RepositoryControlError::InvalidPersistedMember {
                    node_id: self.node_id().clone(),
                    reason: "retired repository cannot be converged",
                })
            }
            RepositoryLifecycle::Retired if !self.ready_at_is_stable() => {
                Err(RepositoryControlError::InvalidPersistedMember {
                    node_id: self.node_id().clone(),
                    reason: "retired repository must have been ready after stable catch-up",
                })
            }
            _ => Ok(()),
        }
    }

    fn ready_at_is_stable(&self) -> bool {
        self.catch_up_completed_at
            .zip(self.ready_at)
            .and_then(|(catch_up_completed_at, ready_at)| {
                ready_at.checked_sub(catch_up_completed_at)
            })
            .is_some_and(|elapsed_seconds| elapsed_seconds >= READY_STABILITY_WINDOW_SECONDS)
    }
}

impl<'de> Deserialize<'de> for RepositoryMember {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawRepositoryMember {
            identity: RepositoryNodeIdentity,
            #[serde(default)]
            lifecycle: RepositoryLifecycle,
            #[serde(default)]
            catch_up_completed_at: Option<u64>,
            #[serde(default)]
            ready_at: Option<u64>,
            #[serde(default)]
            replica_converged: bool,
            #[serde(default)]
            capacity: RepositoryCapacity,
        }

        let raw = RawRepositoryMember::deserialize(deserializer)?;
        let member = Self {
            identity: raw.identity,
            lifecycle: raw.lifecycle,
            catch_up_completed_at: raw.catch_up_completed_at,
            ready_at: raw.ready_at,
            replica_converged: raw.replica_converged,
            capacity: raw.capacity,
        };
        member.validate().map_err(D::Error::custom)?;
        Ok(member)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct RepositoryMembership {
    members: Vec<RepositoryMember>,
}

impl RepositoryMembership {
    pub(crate) fn new(mut members: Vec<RepositoryMember>) -> Result<Self, RepositoryControlError> {
        if members.is_empty() {
            return Err(RepositoryControlError::EmptyMembership);
        }
        members.sort_by(|left, right| left.node_id().cmp(right.node_id()));
        for member in &members {
            member.validate()?;
        }
        for pair in members.windows(2) {
            if pair[0].node_id() == pair[1].node_id() {
                return Err(RepositoryControlError::DuplicateRepositoryMember {
                    node_id: pair[0].node_id().clone(),
                });
            }
        }

        Ok(Self { members })
    }

    pub(crate) fn members(&self) -> &[RepositoryMember] {
        &self.members
    }

    pub(crate) fn add_repository(
        &mut self,
        member: RepositoryMember,
    ) -> Result<(), RepositoryControlError> {
        member.validate()?;
        match self
            .members
            .binary_search_by(|existing| existing.node_id().cmp(member.node_id()))
        {
            Ok(_) => Err(RepositoryControlError::DuplicateRepositoryMember {
                node_id: member.node_id().clone(),
            }),
            Err(index) => {
                self.members.insert(index, member);
                Ok(())
            }
        }
    }

    pub(crate) fn repository(&self, node_id: &RepositoryNodeId) -> Option<&RepositoryMember> {
        self.members
            .binary_search_by(|member| member.node_id().cmp(node_id))
            .ok()
            .map(|index| &self.members[index])
    }

    pub(crate) fn mark_catch_up_complete(
        &mut self,
        node_id: &RepositoryNodeId,
        completed_at: u64,
    ) -> Result<(), RepositoryControlError> {
        let member = self.repository_mut(node_id)?;
        if member.lifecycle != RepositoryLifecycle::Syncing {
            return Err(RepositoryControlError::InvalidLifecycleTransition {
                node_id: node_id.clone(),
                from: member.lifecycle,
                action: "complete catch-up",
            });
        }
        if member
            .catch_up_completed_at
            .is_some_and(|previous| completed_at < previous)
        {
            return Err(RepositoryControlError::ClockBeforeCatchUp {
                node_id: node_id.clone(),
            });
        }
        member.catch_up_completed_at = Some(completed_at);
        member.ready_at = None;
        Ok(())
    }

    pub(crate) fn mark_ready(
        &mut self,
        node_id: &RepositoryNodeId,
        ready_at: u64,
    ) -> Result<(), RepositoryControlError> {
        let member = self.repository_mut(node_id)?;
        if member.lifecycle != RepositoryLifecycle::Syncing {
            return Err(RepositoryControlError::InvalidLifecycleTransition {
                node_id: node_id.clone(),
                from: member.lifecycle,
                action: "become ready",
            });
        }
        let completed_at = member.catch_up_completed_at.ok_or_else(|| {
            RepositoryControlError::CatchUpNotComplete {
                node_id: node_id.clone(),
            }
        })?;
        let elapsed_seconds = ready_at.checked_sub(completed_at).ok_or_else(|| {
            RepositoryControlError::ClockBeforeCatchUp {
                node_id: node_id.clone(),
            }
        })?;
        if elapsed_seconds < READY_STABILITY_WINDOW_SECONDS {
            return Err(RepositoryControlError::StabilityWindowIncomplete {
                node_id: node_id.clone(),
                required_seconds: READY_STABILITY_WINDOW_SECONDS,
                elapsed_seconds,
            });
        }

        member.lifecycle = RepositoryLifecycle::Ready;
        member.ready_at = Some(ready_at);
        Ok(())
    }

    pub(crate) fn set_replica_converged(
        &mut self,
        node_id: &RepositoryNodeId,
        replica_converged: bool,
    ) -> Result<(), RepositoryControlError> {
        let member = self.repository_mut(node_id)?;
        if member.lifecycle != RepositoryLifecycle::Ready {
            return Err(RepositoryControlError::InvalidLifecycleTransition {
                node_id: node_id.clone(),
                from: member.lifecycle,
                action: "change replica convergence",
            });
        }
        member.replica_converged = replica_converged;
        Ok(())
    }

    pub(crate) fn record_capacity(
        &mut self,
        node_id: &RepositoryNodeId,
        used_bytes: u64,
        filesystem_available_bytes: u64,
    ) -> Result<(), RepositoryControlError> {
        let member = self.repository_mut(node_id)?;
        member
            .capacity
            .record_usage(used_bytes, filesystem_available_bytes)?;
        Ok(())
    }

    pub(crate) fn retire(
        &mut self,
        node_id: &RepositoryNodeId,
        decision: RetirementDecision,
    ) -> Result<(), RepositoryControlError> {
        let target_index = self.repository_index(node_id)?;
        let target = &self.members[target_index];
        if target.lifecycle != RepositoryLifecycle::Ready {
            return Err(RepositoryControlError::InvalidLifecycleTransition {
                node_id: node_id.clone(),
                from: target.lifecycle,
                action: "retire",
            });
        }
        if let RetirementDecision::Ordinary = &decision {
            let active_members = self
                .members
                .iter()
                .filter(|member| member.lifecycle != RepositoryLifecycle::Retired)
                .count();
            if active_members == 1 {
                return Err(
                    RepositoryControlError::LastRepositoryRequiresEmergencyDecision {
                        node_id: node_id.clone(),
                    },
                );
            }
            let replacement_exists = self.members.iter().enumerate().any(|(index, member)| {
                index != target_index
                    && member.lifecycle == RepositoryLifecycle::Ready
                    && member.replica_converged
            });
            if !replacement_exists {
                return Err(RepositoryControlError::NoReadyConvergedReplacement {
                    node_id: node_id.clone(),
                });
            }
        }
        if let RetirementDecision::ForceEmergency { reason } = &decision
            && reason.trim().is_empty()
        {
            return Err(RepositoryControlError::EmergencyReasonRequired);
        }

        let member = &mut self.members[target_index];
        member.lifecycle = RepositoryLifecycle::Retired;
        member.replica_converged = false;
        Ok(())
    }

    fn repository_index(
        &self,
        node_id: &RepositoryNodeId,
    ) -> Result<usize, RepositoryControlError> {
        self.members
            .binary_search_by(|member| member.node_id().cmp(node_id))
            .map_err(|_| RepositoryControlError::MissingRepository {
                node_id: node_id.clone(),
            })
    }

    fn repository_mut(
        &mut self,
        node_id: &RepositoryNodeId,
    ) -> Result<&mut RepositoryMember, RepositoryControlError> {
        let index = self.repository_index(node_id)?;
        Ok(&mut self.members[index])
    }
}

impl<'de> Deserialize<'de> for RepositoryMembership {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawMembership {
            members: Vec<RepositoryMember>,
        }

        Self::new(RawMembership::deserialize(deserializer)?.members).map_err(D::Error::custom)
    }
}
