use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    sync::OnceLock,
    time::Duration,
};

use chrono::Utc;
use sha2::{Digest as _, Sha256};
use tokio::sync::Mutex;

use crate::{
    raft::{
        app::RaftFacade,
        types::{NodeId, NodeMeta, raft_node_id_from_ulid},
    },
    state::{
        DesiredStateCommand, MembershipOperation, MembershipOperationKind, MembershipOperationPhase,
    },
};

mod remove_node_operation;
pub(crate) mod stale_learner_recovery;
mod stale_learner_retirement;
mod unreachable_voter_eviction;
pub use stale_learner_retirement::{
    StaleLearnerRetirementPreview, begin_stale_learner_retirement, preview_stale_learner_retirement,
};
pub use unreachable_voter_eviction::{
    UnreachableVoterEvictionPreview, begin_unreachable_voter_eviction,
    preview_unreachable_voter_eviction,
};

pub use crate::raft_membership_cleanup::{
    MembershipRemovalCleanup, finalize_remove_node_cleanup_once,
};
static MEMBERSHIP_OPERATION_GATE: OnceLock<Arc<Mutex<()>>> = OnceLock::new();
pub fn membership_operation_gate() -> Arc<Mutex<()>> {
    MEMBERSHIP_OPERATION_GATE
        .get_or_init(|| Arc::new(Mutex::new(())))
        .clone()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MembershipAudit {
    pub orphan_voters: BTreeSet<NodeId>,
    pub duplicate_desired_members: BTreeSet<NodeId>,
    pub unexpected_learners: BTreeSet<NodeId>,
    pub missing_desired_members: BTreeSet<NodeId>,
}

impl MembershipAudit {
    pub fn is_clean(&self) -> bool {
        self.orphan_voters.is_empty()
            && self.duplicate_desired_members.is_empty()
            && self.unexpected_learners.is_empty()
            && self.missing_desired_members.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanVoterRepairPreview {
    pub raft_node_id: NodeId,
    pub expected_membership: String,
}

pub fn membership_revision(
    metrics: &openraft::RaftMetrics<NodeId, NodeMeta>,
) -> anyhow::Result<String> {
    let membership = metrics.membership_config.membership();
    let voters = membership.voter_ids().collect::<BTreeSet<_>>();
    let mut nodes = metrics
        .membership_config
        .nodes()
        .map(|(node_id, node)| {
            (
                *node_id,
                serde_json::json!({
                    "name": node.name,
                    "api_base_url": node.api_base_url,
                    "raft_endpoint": node.raft_endpoint,
                }),
            )
        })
        .collect::<Vec<_>>();
    nodes.sort_by_key(|(node_id, _)| *node_id);
    let joint_configs = membership
        .get_joint_config()
        .iter()
        .map(|config| config.iter().copied().collect::<BTreeSet<_>>())
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&serde_json::json!({
        "log_id": format!("{:?}", metrics.membership_config.log_id()),
        "voters": voters,
        "joint_configs": joint_configs,
        "nodes": nodes,
    }))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

pub async fn audit_membership(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<crate::state::JsonSnapshotStore>>,
) -> MembershipAudit {
    let metrics = raft.metrics().borrow().clone();
    let voters = metrics
        .membership_config
        .membership()
        .voter_ids()
        .collect::<BTreeSet<_>>();
    let (desired_nodes, duplicate_desired_members, pending_join_nodes, active_operation) = {
        let store = store.lock().await;
        let state = store.state();
        let desired_member_counts = state
            .nodes
            .keys()
            .filter_map(|node_id| raft_node_id_from_ulid(node_id).ok())
            .fold(BTreeMap::new(), |mut counts, node_id| {
                *counts.entry(node_id).or_insert(0usize) += 1;
                counts
            });
        let desired_nodes = desired_member_counts
            .keys()
            .copied()
            .collect::<BTreeSet<_>>();
        let duplicate_desired_members = desired_member_counts
            .into_iter()
            .filter_map(|(node_id, count)| (count > 1).then_some(node_id))
            .collect::<BTreeSet<_>>();
        let pending_join_nodes = state
            .join_sessions
            .values()
            .filter(|session| session.status.is_pending())
            .filter_map(|session| raft_node_id_from_ulid(&session.node_id).ok())
            .collect::<BTreeSet<_>>();
        let active_operation = state.active_membership_operation().cloned();
        (
            desired_nodes,
            duplicate_desired_members,
            pending_join_nodes,
            active_operation,
        )
    };
    let orphan_voters = voters
        .iter()
        .filter(|node_id| !desired_nodes.contains(node_id))
        .copied()
        .collect();
    let permitted_learner = active_operation.as_ref().and_then(|operation| {
        let phase_allows_learner = matches!(
            operation.phase,
            MembershipOperationPhase::Prepared | MembershipOperationPhase::LearnerRegistered
        );
        match operation.kind {
            MembershipOperationKind::Join
                if phase_allows_learner && pending_join_nodes.contains(&operation.raft_node_id) =>
            {
                Some(operation.raft_node_id)
            }
            MembershipOperationKind::Restore if phase_allows_learner => {
                Some(operation.raft_node_id)
            }
            MembershipOperationKind::RemoveNode
                if operation.remove_learner
                    && operation.phase == MembershipOperationPhase::Prepared =>
            {
                Some(operation.raft_node_id)
            }
            _ => None,
        }
    });
    let unexpected_learners = metrics
        .membership_config
        .nodes()
        .filter_map(|(node_id, _)| {
            if voters.contains(node_id) || permitted_learner == Some(*node_id) {
                None
            } else {
                Some(*node_id)
            }
        })
        .collect();
    let membership_nodes = metrics
        .membership_config
        .nodes()
        .map(|(node_id, _)| *node_id)
        .collect::<BTreeSet<_>>();
    let missing_desired_members = desired_nodes
        .iter()
        .filter(|node_id| !membership_nodes.contains(node_id))
        .filter(|node_id| {
            !active_operation.as_ref().is_some_and(|operation| {
                operation.raft_node_id == **node_id
                    && matches!(
                        (&operation.kind, &operation.phase),
                        (
                            MembershipOperationKind::Join | MembershipOperationKind::Restore,
                            MembershipOperationPhase::Prepared
                        ) | (
                            MembershipOperationKind::RemoveNode,
                            MembershipOperationPhase::MembershipRemoved
                        )
                    )
            })
        })
        .copied()
        .collect();
    MembershipAudit {
        orphan_voters,
        duplicate_desired_members,
        unexpected_learners,
        missing_desired_members,
    }
}

pub async fn require_clean_membership_for_write(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<crate::state::JsonSnapshotStore>>,
) -> anyhow::Result<()> {
    raft.ensure_linearizable().await?;
    let audit = audit_membership(raft, store).await;
    if audit.is_clean() {
        return Ok(());
    }
    Err(membership_invariant_error(&audit))
}

/// A delete may begin only from a clean, mapped-voter membership state. Recovery of an already
/// recorded delete is handled by the lifecycle resumer, never by accepting a new delete against
/// an absent or learner target.
pub async fn require_clean_membership_for_remove_node(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<crate::state::JsonSnapshotStore>>,
) -> anyhow::Result<()> {
    require_clean_membership_for_write(raft, store).await
}
pub async fn require_clean_membership_for_restore_node(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<crate::state::JsonSnapshotStore>>,
    raft_node_id: NodeId,
    allowed_missing: &BTreeSet<NodeId>,
) -> anyhow::Result<()> {
    raft.ensure_linearizable().await?;
    let mut audit = audit_membership(raft, store).await;
    audit.missing_desired_members.remove(&raft_node_id);
    if audit.missing_desired_members != *allowed_missing {
        anyhow::bail!("membership_invariant_violation: restore allowlist mismatch");
    }
    audit.missing_desired_members.clear();
    audit
        .is_clean()
        .then_some(())
        .ok_or_else(|| membership_invariant_error(&audit))
}
pub(crate) fn membership_invariant_error(audit: &MembershipAudit) -> anyhow::Error {
    anyhow::anyhow!(
        "membership_invariant_violation: orphan_voters={:?}, duplicate_desired_members={:?}, \
         unexpected_learners={:?}, missing_desired_members={:?}",
        audit.orphan_voters,
        audit.duplicate_desired_members,
        audit.unexpected_learners,
        audit.missing_desired_members,
    )
}

pub async fn preview_orphan_voter_repair(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<crate::state::JsonSnapshotStore>>,
    raft_node_id: NodeId,
) -> anyhow::Result<OrphanVoterRepairPreview> {
    raft.ensure_linearizable().await?;
    let metrics = raft.metrics().borrow().clone();
    validate_orphan_voter_repair(&metrics, &store, raft_node_id, None).await?;
    Ok(OrphanVoterRepairPreview {
        raft_node_id,
        expected_membership: membership_revision(&metrics)?,
    })
}

pub async fn repair_orphan_voter(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<crate::state::JsonSnapshotStore>>,
    raft_node_id: NodeId,
    expected_membership: &str,
) -> anyhow::Result<MembershipOperation> {
    let _guard = membership_operation_gate().lock_owned().await;
    raft.ensure_linearizable().await?;
    let metrics = raft.metrics().borrow().clone();
    validate_orphan_voter_repair(&metrics, &store, raft_node_id, Some(expected_membership)).await?;

    let operation = MembershipOperation {
        operation_id: uuid::Uuid::new_v4().to_string(),
        kind: MembershipOperationKind::RepairOrphanVoter,
        raft_node_id,
        node_id: None,
        expected_membership: expected_membership.to_string(),
        phase: MembershipOperationPhase::Prepared,
        legacy: false,
        remove_learner: false,
        delete_endpoints: false,
        expected_endpoint_ids: Vec::new(),
        expected_endpoint_tags: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        next_retry_at: None,
        terminal_at: None,
        evidence: Some("manual orphan voter repair".to_string()),
    };
    write_operation(
        &raft,
        DesiredStateCommand::BeginMembershipOperation {
            operation: Box::new(operation.clone()),
            node: None,
            join_session: None,
        },
    )
    .await?;

    if let Err(error) = require_expected_membership(&raft, &operation).await {
        block_membership_operation(&raft, operation.clone(), error.to_string()).await?;
        anyhow::bail!("membership revision changed before orphan voter repair")
    }

    raft.change_membership(
        openraft::ChangeMembers::RemoveVoters(BTreeSet::from([raft_node_id])),
        false,
    )
    .await?;

    let after = raft.metrics().borrow().clone();
    if after
        .membership_config
        .membership()
        .get_node(&raft_node_id)
        .is_some()
    {
        anyhow::bail!("orphan voter removal did not remove node from membership")
    }
    let membership_removed = MembershipOperation {
        expected_membership: membership_revision(&after)?,
        phase: MembershipOperationPhase::MembershipRemoved,
        evidence: Some("orphan voter removed with RemoveVoters retain=false".to_string()),
        ..operation
    };
    write_operation(
        &raft,
        DesiredStateCommand::TransitionMembershipOperation {
            operation: membership_removed.clone(),
        },
    )
    .await?;
    let completed = MembershipOperation {
        phase: MembershipOperationPhase::Completed,
        terminal_at: Some(Utc::now().to_rfc3339()),
        evidence: Some("orphan voter repair completed".to_string()),
        ..membership_removed
    };
    write_operation(
        &raft,
        DesiredStateCommand::TransitionMembershipOperation {
            operation: completed.clone(),
        },
    )
    .await?;
    Ok(completed)
}

async fn resume_orphan_voter_repair_operation(
    raft: &Arc<dyn RaftFacade>,
    store: &Arc<Mutex<crate::state::JsonSnapshotStore>>,
    operation: MembershipOperation,
) -> anyhow::Result<()> {
    let metrics = raft.metrics().borrow().clone();
    if !is_current_local_leader(&metrics) {
        return Ok(());
    }
    if metrics
        .membership_config
        .membership()
        .get_joint_config()
        .len()
        > 1
    {
        block_membership_operation(
            raft,
            operation,
            "orphan voter repair cannot resume during joint consensus",
        )
        .await?;
        return Ok(());
    }

    if operation.phase == MembershipOperationPhase::MembershipRemoved {
        let _ = transition_operation(
            raft,
            operation,
            MembershipOperationPhase::Completed,
            "orphan voter repair completed after recovery",
        )
        .await?;
        return Ok(());
    }
    if operation.phase != MembershipOperationPhase::Prepared {
        block_membership_operation(
            raft,
            operation,
            "orphan voter repair has an invalid resumable phase",
        )
        .await?;
        return Ok(());
    }

    let membership = metrics.membership_config.membership();
    let target_is_voter = membership
        .voter_ids()
        .any(|node_id| node_id == operation.raft_node_id);
    let target_exists = membership.get_node(&operation.raft_node_id).is_some();
    if metrics.id == operation.raft_node_id {
        block_membership_operation(
            raft,
            operation,
            "orphan voter repair target became the current leader",
        )
        .await?;
        return Ok(());
    }

    let (desired_nodes, target_has_pending_session) = {
        let store = store.lock().await;
        let desired_nodes = store
            .state()
            .nodes
            .keys()
            .filter_map(|node_id| raft_node_id_from_ulid(node_id).ok())
            .collect::<BTreeSet<_>>();
        let target_has_pending_session = store
            .state()
            .join_sessions
            .values()
            .filter(|session| session.status.is_pending())
            .filter_map(|session| raft_node_id_from_ulid(&session.node_id).ok())
            .any(|node_id| node_id == operation.raft_node_id);
        (desired_nodes, target_has_pending_session)
    };
    if desired_nodes.contains(&operation.raft_node_id) || target_has_pending_session {
        block_membership_operation(
            raft,
            operation,
            "orphan voter repair target gained desired-state ownership",
        )
        .await?;
        return Ok(());
    }

    if !target_exists {
        // This is the exact postcondition of the one RemoveVoters request. It covers the crash
        // window after Raft commits the removal but before the operation transition is durable.
        let operation = transition_operation(
            raft,
            operation,
            MembershipOperationPhase::MembershipRemoved,
            "orphan voter absent after uncertain RemoveVoters request",
        )
        .await?;
        let _ = transition_operation(
            raft,
            operation,
            MembershipOperationPhase::Completed,
            "orphan voter repair completed after uncertain request",
        )
        .await?;
        return Ok(());
    }
    if !target_is_voter {
        block_membership_operation(
            raft,
            operation,
            "orphan voter repair target is an unexpected learner",
        )
        .await?;
        return Ok(());
    }

    let orphan_voters = membership
        .voter_ids()
        .filter(|node_id| !desired_nodes.contains(node_id))
        .collect::<BTreeSet<_>>();
    if orphan_voters != BTreeSet::from([operation.raft_node_id]) {
        block_membership_operation(
            raft,
            operation,
            "orphan voter repair target is no longer the unique orphan voter",
        )
        .await?;
        return Ok(());
    }
    if let Err(error) = require_expected_membership(raft, &operation).await {
        block_membership_operation(raft, operation, error.to_string()).await?;
        return Ok(());
    }

    raft.change_membership(
        openraft::ChangeMembers::RemoveVoters(BTreeSet::from([operation.raft_node_id])),
        false,
    )
    .await?;
    let after = raft.metrics().borrow().clone();
    if after
        .membership_config
        .membership()
        .get_node(&operation.raft_node_id)
        .is_some()
    {
        anyhow::bail!("orphan voter repair did not make target absent")
    }
    let operation = transition_operation(
        raft,
        operation,
        MembershipOperationPhase::MembershipRemoved,
        "orphan voter removed with RemoveVoters retain=false",
    )
    .await?;
    let _ = transition_operation(
        raft,
        operation,
        MembershipOperationPhase::Completed,
        "orphan voter repair completed",
    )
    .await?;
    Ok(())
}

async fn validate_orphan_voter_repair(
    metrics: &openraft::RaftMetrics<NodeId, NodeMeta>,
    store: &Arc<Mutex<crate::state::JsonSnapshotStore>>,
    raft_node_id: NodeId,
    expected_membership: Option<&str>,
) -> anyhow::Result<()> {
    if !is_current_local_leader(metrics) {
        anyhow::bail!("orphan voter repair must run on the current local leader")
    }
    let membership = metrics.membership_config.membership();
    if membership.get_joint_config().len() > 1 {
        anyhow::bail!("orphan voter repair is forbidden during joint consensus")
    }
    if metrics.id == raft_node_id {
        anyhow::bail!("orphan voter repair cannot remove the current leader")
    }
    if !membership
        .voter_ids()
        .any(|node_id| node_id == raft_node_id)
    {
        anyhow::bail!("repair target is not a current voter")
    }
    if let Some(expected_membership) = expected_membership
        && membership_revision(metrics)? != expected_membership
    {
        anyhow::bail!("membership revision changed since dry-run")
    }
    let store = store.lock().await;
    if store.state().active_membership_operation().is_some() {
        anyhow::bail!("another membership operation is active")
    }
    let desired_nodes = store
        .state()
        .nodes
        .keys()
        .filter_map(|node_id| raft_node_id_from_ulid(node_id).ok())
        .collect::<BTreeSet<_>>();
    if desired_nodes.contains(&raft_node_id) {
        anyhow::bail!("repair target is represented by desired state")
    }
    let orphan_voters = membership
        .voter_ids()
        .filter(|node_id| !desired_nodes.contains(node_id))
        .collect::<BTreeSet<_>>();
    if orphan_voters != BTreeSet::from([raft_node_id]) {
        anyhow::bail!("repair target must be the unique orphan voter")
    }
    if store
        .state()
        .join_sessions
        .values()
        .filter(|session| session.status.is_pending())
        .filter_map(|session| raft_node_id_from_ulid(&session.node_id).ok())
        .any(|node_id| node_id == raft_node_id)
    {
        anyhow::bail!("repair target has a pending join session")
    }
    Ok(())
}

pub(crate) async fn write_operation(
    raft: &Arc<dyn RaftFacade>,
    command: DesiredStateCommand,
) -> anyhow::Result<()> {
    match raft.client_write(command).await? {
        crate::raft::types::ClientResponse::Ok { .. } => Ok(()),
        crate::raft::types::ClientResponse::Err { code, message, .. } => {
            anyhow::bail!("membership operation state write rejected: {code}: {message}")
        }
    }
}

pub(crate) async fn transition_operation(
    raft: &Arc<dyn RaftFacade>,
    mut operation: MembershipOperation,
    phase: MembershipOperationPhase,
    evidence: &str,
) -> anyhow::Result<MembershipOperation> {
    operation.expected_membership = membership_revision(&raft.metrics().borrow().clone())?;
    operation.phase = phase;
    operation.evidence = Some(evidence.to_string());
    if operation.phase.is_terminal() {
        operation.terminal_at = Some(Utc::now().to_rfc3339());
    }
    write_operation(
        raft,
        DesiredStateCommand::TransitionMembershipOperation {
            operation: operation.clone(),
        },
    )
    .await?;
    Ok(operation)
}

pub async fn block_membership_operation(
    raft: &Arc<dyn RaftFacade>,
    mut operation: MembershipOperation,
    evidence: impl Into<String>,
) -> anyhow::Result<()> {
    operation.phase = MembershipOperationPhase::Blocked;
    operation.terminal_at = Some(Utc::now().to_rfc3339());
    operation.evidence = Some(evidence.into());
    write_operation(
        raft,
        DesiredStateCommand::TransitionMembershipOperation { operation },
    )
    .await
}

pub(crate) fn is_current_local_leader(metrics: &openraft::RaftMetrics<NodeId, NodeMeta>) -> bool {
    metrics.state == openraft::ServerState::Leader && metrics.current_leader == Some(metrics.id)
}

pub(crate) async fn require_expected_membership(
    raft: &Arc<dyn RaftFacade>,
    operation: &MembershipOperation,
) -> anyhow::Result<openraft::RaftMetrics<NodeId, NodeMeta>> {
    raft.ensure_linearizable().await?;
    let metrics = raft.metrics().borrow().clone();
    let current = membership_revision(&metrics)?;
    if current != operation.expected_membership {
        anyhow::bail!(
            "membership revision changed for operation {}",
            operation.operation_id
        )
    }
    Ok(metrics)
}

async fn prune_terminal_operations(
    raft: &Arc<dyn RaftFacade>,
    store: &Arc<Mutex<crate::state::JsonSnapshotStore>>,
) -> anyhow::Result<()> {
    let has_terminal_operation = store
        .lock()
        .await
        .state()
        .membership_operations
        .values()
        .any(|operation| operation.phase.is_terminal());
    if !has_terminal_operation {
        return Ok(());
    }
    let metrics = raft.metrics().borrow().clone();
    if !is_current_local_leader(&metrics) {
        return Ok(());
    }
    raft.ensure_linearizable().await?;
    write_operation(
        raft,
        DesiredStateCommand::PruneMembershipOperations {
            terminal_before: (Utc::now() - chrono::Duration::hours(24)).to_rfc3339(),
        },
    )
    .await
}

async fn resume_restore_operation(
    raft: &Arc<dyn RaftFacade>,
    store: &Arc<Mutex<crate::state::JsonSnapshotStore>>,
    operation: MembershipOperation,
) -> anyhow::Result<()> {
    let node_id = operation
        .node_id
        .clone()
        .ok_or_else(|| anyhow::anyhow!("restore operation is missing node_id"))?;
    let node = store
        .lock()
        .await
        .get_node(&node_id)
        .ok_or_else(|| anyhow::anyhow!("restore operation node is missing"))?;
    let metrics = raft.metrics().borrow().clone();
    if !is_current_local_leader(&metrics) {
        return Ok(());
    }
    let membership = metrics.membership_config.membership();
    let target_is_voter = membership
        .voter_ids()
        .any(|node_id| node_id == operation.raft_node_id);
    let target_exists = membership.get_node(&operation.raft_node_id).is_some();

    match operation.phase {
        MembershipOperationPhase::Prepared => {
            if target_is_voter {
                block_membership_operation(
                    raft,
                    operation,
                    "restore target unexpectedly became voter before learner phase",
                )
                .await?;
                return Ok(());
            }
            if target_exists {
                let _ = transition_operation(
                    raft,
                    operation,
                    MembershipOperationPhase::LearnerRegistered,
                    "restore learner observed after uncertain request",
                )
                .await?;
                return Ok(());
            }
            if let Err(error) = require_expected_membership(raft, &operation).await {
                block_membership_operation(raft, operation, error.to_string()).await?;
                return Ok(());
            }
            raft.add_learner(
                operation.raft_node_id,
                NodeMeta {
                    name: node.node_name,
                    api_base_url: node.api_base_url.clone(),
                    raft_endpoint: node.api_base_url,
                },
            )
            .await?;
            let after = raft.metrics().borrow().clone();
            if after
                .membership_config
                .membership()
                .get_node(&operation.raft_node_id)
                .is_none()
            {
                // OpenRaft acknowledges the change before the local metrics watcher is required
                // to publish it. Leave the durable intent in Prepared; the next leader scan will
                // advance only after it observes the exact learner shape.
                return Ok(());
            }
            let _ = transition_operation(
                raft,
                operation,
                MembershipOperationPhase::LearnerRegistered,
                "restore learner registered",
            )
            .await?;
        }
        MembershipOperationPhase::LearnerRegistered => {
            if target_is_voter {
                let _ = transition_operation(
                    raft,
                    operation,
                    MembershipOperationPhase::VoterPromoted,
                    "restore voter observed after uncertain request",
                )
                .await?;
                return Ok(());
            }
            if !target_exists {
                block_membership_operation(
                    raft,
                    operation,
                    "restore learner disappeared before promotion",
                )
                .await?;
                return Ok(());
            }
            let metrics = match require_expected_membership(raft, &operation).await {
                Ok(metrics) => metrics,
                Err(error) => {
                    block_membership_operation(raft, operation, error.to_string()).await?;
                    return Ok(());
                }
            };
            let required_log_index = metrics.last_log_index.unwrap_or(0);
            if raft
                .wait_learner_caught_up(
                    operation.raft_node_id,
                    required_log_index,
                    Duration::from_secs(30),
                )
                .await
                .is_err()
            {
                return Ok(());
            }
            raft.add_voters(BTreeSet::from([operation.raft_node_id]))
                .await?;
            let after = raft.metrics().borrow().clone();
            if !after
                .membership_config
                .membership()
                .voter_ids()
                .any(|node_id| node_id == operation.raft_node_id)
            {
                // As above, wait for the confirmed metrics shape before recording promotion.
                return Ok(());
            }
            let _ = transition_operation(
                raft,
                operation,
                MembershipOperationPhase::VoterPromoted,
                "restore voter promoted",
            )
            .await?;
        }
        MembershipOperationPhase::VoterPromoted => {
            if !target_is_voter {
                block_membership_operation(
                    raft,
                    operation,
                    "restore voter disappeared before completion",
                )
                .await?;
                return Ok(());
            }
            let _ = transition_operation(
                raft,
                operation,
                MembershipOperationPhase::Completed,
                "restore completed",
            )
            .await?;
        }
        _ => {}
    }
    Ok(())
}

/// Continue only a durable, explicitly recorded operation. Unexpected membership shapes are
/// errors, never an instruction to infer a promotion or rollback.
pub async fn resume_membership_operations_once(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<crate::state::JsonSnapshotStore>>,
) -> anyhow::Result<()> {
    let Ok(_guard) = membership_operation_gate().try_lock_owned() else {
        return Ok(());
    };
    let operation = store
        .lock()
        .await
        .state()
        .active_membership_operation()
        .cloned();
    let Some(operation) = operation else {
        return prune_terminal_operations(&raft, &store).await;
    };
    match operation.kind {
        MembershipOperationKind::Restore => {
            return resume_restore_operation(&raft, &store, operation).await;
        }
        MembershipOperationKind::RepairOrphanVoter => {
            return resume_orphan_voter_repair_operation(&raft, &store, operation).await;
        }
        MembershipOperationKind::RemoveNode => {
            return remove_node_operation::resume_remove_node_operation(&raft, &store, operation)
                .await;
        }
        _ => Ok(()),
    }
}

/// The periodic task resumes only an already durable lifecycle operation; it never infers a new
/// membership action from an audit finding.
pub fn spawn_membership_voter_guard(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<crate::state::JsonSnapshotStore>>,
    cleanup: MembershipRemovalCleanup,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            if let Err(error) = resume_membership_operations_once(raft.clone(), store.clone()).await
            {
                tracing::warn!(%error, "membership operation resume failed");
            }
            if let Err(error) =
                finalize_remove_node_cleanup_once(raft.clone(), store.clone(), &cleanup).await
            {
                tracing::warn!(%error, "membership operation cleanup failed");
            }
            let audit = audit_membership(raft.clone(), store.clone()).await;
            if !audit.is_clean() {
                tracing::error!(
                    orphan_voters = ?audit.orphan_voters,
                    duplicate_desired_members = ?audit.duplicate_desired_members,
                    unexpected_learners = ?audit.unexpected_learners,
                    missing_desired_members = ?audit.missing_desired_members,
                    "membership_invariant_violation; automatic membership repair is disabled"
                );
            }
            tokio::time::sleep(interval).await;
        }
    })
}
