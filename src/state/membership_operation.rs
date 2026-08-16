use serde::{Deserialize, Serialize};

use super::{DesiredStateApplyResult, DesiredStateCommand, PersistedState, StoreError};

/// The only supported sources of a membership change. The role itself is represented by
/// OpenRaft; this record makes the intent and recovery boundary durable in desired state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MembershipOperationKind {
    Join,
    Restore,
    RemoveNode,
    RepairOrphanVoter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MembershipOperationPhase {
    Prepared,
    LearnerRegistered,
    VoterPromoted,
    MembershipRemoved,
    Completed,
    Blocked,
    Expired,
}

impl MembershipOperationPhase {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Blocked | Self::Expired)
    }

    fn may_transition_to(&self, next: &Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Prepared,
                Self::Prepared
                    | Self::LearnerRegistered
                    | Self::MembershipRemoved
                    | Self::Blocked
                    | Self::Expired
            ) | (
                Self::LearnerRegistered,
                Self::LearnerRegistered | Self::VoterPromoted | Self::Blocked | Self::Expired
            ) | (
                Self::VoterPromoted,
                Self::VoterPromoted | Self::Completed | Self::Blocked
            ) | (
                Self::MembershipRemoved,
                Self::MembershipRemoved | Self::Completed | Self::Blocked
            ) | (Self::Completed, Self::Completed)
                | (Self::Blocked, Self::Blocked)
                | (Self::Expired, Self::Expired)
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MembershipOperation {
    pub operation_id: String,
    pub kind: MembershipOperationKind,
    pub raft_node_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    /// Opaque precondition generated from the linearizable membership view.
    pub expected_membership: String,
    pub phase: MembershipOperationPhase,
    #[serde(default)]
    pub legacy: bool,
    #[serde(default)]
    pub delete_endpoints: bool,
    #[serde(default)]
    pub expected_endpoint_ids: Vec<String>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_retry_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

impl MembershipOperation {
    pub fn is_active(&self) -> bool {
        !self.phase.is_terminal()
    }

    pub(crate) fn validate_successor(&self, next: &Self) -> Result<(), &'static str> {
        if self.operation_id != next.operation_id
            || self.kind != next.kind
            || self.raft_node_id != next.raft_node_id
            || self.node_id != next.node_id
            || self.created_at != next.created_at
            || self.legacy != next.legacy
            || self.delete_endpoints != next.delete_endpoints
            || self.expected_endpoint_ids != next.expected_endpoint_ids
        {
            return Err("immutable membership operation identity changed");
        }
        if !self.phase.may_transition_to(&next.phase) {
            return Err("invalid membership operation phase transition");
        }
        if next.phase.is_terminal() != next.terminal_at.is_some() {
            return Err("terminal membership operation must have terminal_at");
        }
        if next.phase.is_terminal()
            && next
                .evidence
                .as_deref()
                .is_none_or(|evidence| evidence.trim().is_empty())
        {
            return Err("terminal membership operation must have evidence");
        }
        if next.phase.is_terminal()
            && next
                .terminal_at
                .as_deref()
                .is_none_or(|timestamp| chrono::DateTime::parse_from_rfc3339(timestamp).is_err())
        {
            return Err("terminal membership operation timestamp is invalid");
        }
        Ok(())
    }
}

pub(super) fn apply_command(
    state: &mut PersistedState,
    command: &DesiredStateCommand,
) -> Option<Result<DesiredStateApplyResult, StoreError>> {
    match command {
        DesiredStateCommand::BeginMembershipOperation {
            operation,
            node,
            join_session,
        } => Some((|| {
            let operation = operation.as_ref();
            if operation.operation_id.trim().is_empty()
                || operation.expected_membership.trim().is_empty()
                || operation.created_at.trim().is_empty()
            {
                return Err(StoreError::InvalidMembershipOperation {
                    message: "membership operation identity is empty",
                });
            }
            if operation.phase != MembershipOperationPhase::Prepared {
                return Err(StoreError::InvalidMembershipOperation {
                    message: "membership operation must begin prepared",
                });
            }
            if state
                .membership_operations
                .contains_key(&operation.operation_id)
            {
                return Err(StoreError::InvalidMembershipOperation {
                    message: "membership operation id already exists",
                });
            }
            if state.active_membership_operation().is_some() {
                return Err(StoreError::InvalidMembershipOperation {
                    message: "another membership operation is active",
                });
            }
            if operation.kind == MembershipOperationKind::Join
                && !operation.legacy
                && (node.is_none() || join_session.is_none())
            {
                return Err(StoreError::InvalidMembershipOperation {
                    message: "join operation must atomically persist node and session",
                });
            }
            if node.is_some() != join_session.is_some() {
                return Err(StoreError::InvalidMembershipOperation {
                    message: "membership operation node and join session must be paired",
                });
            }
            // Begin is the durability boundary for a fresh join. Apply against a cloned state so
            // a rejected node/session cannot leave an active orphan operation.
            let mut next = state.clone();
            if let Some(node) = node {
                crate::state_join_command::apply_upsert_node(
                    &mut next,
                    node,
                    join_session.as_ref(),
                )?;
            }
            next.membership_operations
                .insert(operation.operation_id.clone(), operation.clone());
            *state = next;
            Ok(DesiredStateApplyResult::MembershipOperation {
                operation: operation.clone(),
            })
        })()),
        DesiredStateCommand::TransitionMembershipOperation { operation } => Some((|| {
            let current = state
                .membership_operations
                .get(&operation.operation_id)
                .ok_or(StoreError::InvalidMembershipOperation {
                    message: "membership operation does not exist",
                })?;
            current
                .validate_successor(operation)
                .map_err(|message| StoreError::InvalidMembershipOperation { message })?;
            state
                .membership_operations
                .insert(operation.operation_id.clone(), operation.clone());
            Ok(DesiredStateApplyResult::MembershipOperation {
                operation: operation.clone(),
            })
        })()),
        DesiredStateCommand::PruneMembershipOperations { terminal_before } => {
            Some((|| {
                let terminal_before = chrono::DateTime::parse_from_rfc3339(terminal_before)
                    .map_err(|_| StoreError::InvalidMembershipOperation {
                        message: "membership operation prune time is invalid",
                    })?;
                state.membership_operations.retain(|_, operation| {
                    operation.terminal_at.as_ref().is_none_or(|terminal_at| {
                        chrono::DateTime::parse_from_rfc3339(terminal_at)
                            .map_or(true, |terminal_at| terminal_at > terminal_before)
                    })
                });
                Ok(DesiredStateApplyResult::Applied)
            })())
        }
        _ => None,
    }
}
