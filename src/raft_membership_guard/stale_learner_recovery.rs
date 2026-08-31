use std::{collections::BTreeSet, sync::Arc};

use chrono::Utc;
use tokio::sync::Mutex;

use crate::{
    domain::Node,
    join_session::{JoinSession, JoinSessionStatus},
    raft::{
        app::RaftFacade,
        types::{NodeId, NodeMeta, raft_node_id_from_ulid},
    },
    state::{
        DesiredStateCommand, MembershipOperation, MembershipOperationKind, MembershipOperationPhase,
    },
};

use super::{
    audit_membership, is_current_local_leader, membership_operation_gate, membership_revision,
    write_operation,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleLearnerRecoveryPreview {
    pub node_id: String,
    pub raft_node_id: NodeId,
    pub expected_membership: String,
}

struct StaleLearnerRecoveryInput {
    node: Node,
    consumed_join_session: Option<JoinSession>,
}

/// Preview one operator-requested stale learner recovery. The unexpected learner must be the
/// exact requested DesiredState node; a periodic audit never calls this path.
pub async fn preview_stale_learner_recovery(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<crate::state::JsonSnapshotStore>>,
    node_id: &str,
) -> anyhow::Result<StaleLearnerRecoveryPreview> {
    let raft_node_id = raft_node_id_from_ulid(node_id)?;
    raft.ensure_linearizable().await?;
    let metrics = raft.metrics().borrow().clone();
    validate_stale_learner_recovery(&raft, &metrics, &store, node_id, raft_node_id, None).await?;
    Ok(StaleLearnerRecoveryPreview {
        node_id: node_id.to_string(),
        raft_node_id,
        expected_membership: membership_revision(&metrics)?,
    })
}

/// Persist the existing Restore operation shape only after a caller has confirmed the exact
/// linearizable preview. The normal Restore resumer owns catch-up and voter promotion.
pub async fn begin_stale_learner_recovery(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<crate::state::JsonSnapshotStore>>,
    node_id: &str,
    expected_membership: &str,
) -> anyhow::Result<MembershipOperation> {
    let _guard = membership_operation_gate().lock_owned().await;
    let raft_node_id = raft_node_id_from_ulid(node_id)?;
    raft.ensure_linearizable().await?;
    let metrics = raft.metrics().borrow().clone();
    let input = validate_stale_learner_recovery(
        &raft,
        &metrics,
        &store,
        node_id,
        raft_node_id,
        Some(expected_membership),
    )
    .await?;

    let operation = MembershipOperation {
        operation_id: uuid::Uuid::new_v4().to_string(),
        kind: MembershipOperationKind::Restore,
        raft_node_id,
        node_id: Some(node_id.to_string()),
        expected_membership: expected_membership.to_string(),
        phase: MembershipOperationPhase::Prepared,
        legacy: false,
        delete_endpoints: false,
        expected_endpoint_ids: Vec::new(),
        expected_endpoint_tags: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        next_retry_at: None,
        terminal_at: None,
        evidence: Some("manual stale learner recovery".to_string()),
    };
    let (node, join_session) = match input.consumed_join_session {
        Some(join_session) => (Some(input.node), Some(join_session)),
        None => (None, None),
    };
    write_operation(
        &raft,
        DesiredStateCommand::BeginMembershipOperation {
            operation: Box::new(operation.clone()),
            node,
            join_session,
        },
    )
    .await?;
    Ok(operation)
}

async fn validate_stale_learner_recovery(
    raft: &Arc<dyn RaftFacade>,
    metrics: &openraft::RaftMetrics<NodeId, NodeMeta>,
    store: &Arc<Mutex<crate::state::JsonSnapshotStore>>,
    node_id: &str,
    raft_node_id: NodeId,
    expected_membership: Option<&str>,
) -> anyhow::Result<StaleLearnerRecoveryInput> {
    if !is_current_local_leader(metrics) {
        anyhow::bail!("stale learner recovery must run on the current local leader")
    }
    let membership = metrics.membership_config.membership();
    if membership.get_joint_config().len() > 1 {
        anyhow::bail!("stale learner recovery is forbidden during joint consensus")
    }
    if metrics.id == raft_node_id {
        anyhow::bail!("stale learner recovery target cannot be the current leader")
    }
    if membership
        .voter_ids()
        .any(|voter_id| voter_id == raft_node_id)
    {
        anyhow::bail!("stale learner recovery target is already a voter")
    }
    let raft_node = membership.get_node(&raft_node_id).ok_or_else(|| {
        anyhow::anyhow!("stale learner recovery target is absent from membership")
    })?;
    if let Some(expected_membership) = expected_membership
        && membership_revision(metrics)? != expected_membership
    {
        anyhow::bail!("membership revision changed since dry-run")
    }

    let input = {
        let store = store.lock().await;
        if store.state().active_membership_operation().is_some() {
            anyhow::bail!("another membership operation is active")
        }
        let node = store.get_node(node_id).ok_or_else(|| {
            anyhow::anyhow!("stale learner recovery target is not represented by desired state")
        })?;
        if raft_node_id_from_ulid(&node.node_id)? != raft_node_id {
            anyhow::bail!("stale learner recovery target node id does not match Raft id")
        }
        let pending_sessions = store
            .state()
            .join_sessions
            .values()
            .filter(|session| session.status.is_pending())
            .cloned()
            .collect::<Vec<_>>();
        if pending_sessions
            .iter()
            .any(|session| session.node_id != node_id)
        {
            anyhow::bail!("another pending join session exists")
        }
        let consumed_join_session = match pending_sessions.as_slice() {
            [] => None,
            [session] if session.status == JoinSessionStatus::LearnerRegistered => {
                let mut consumed = session.clone();
                consumed.status = JoinSessionStatus::Consumed;
                consumed.terminal_at = Some(Utc::now().to_rfc3339());
                Some(consumed)
            }
            [session] if session.status == JoinSessionStatus::Reserved => {
                anyhow::bail!("stale learner recovery target has a reserved join session")
            }
            _ => anyhow::bail!("stale learner recovery target has ambiguous pending join sessions"),
        };
        StaleLearnerRecoveryInput {
            node,
            consumed_join_session,
        }
    };
    if raft_node.name != input.node.node_name
        || raft_node.api_base_url != input.node.api_base_url
        || raft_node.raft_endpoint != input.node.api_base_url
    {
        anyhow::bail!("stale learner recovery target metadata does not match desired state")
    }

    let audit = audit_membership(raft.clone(), store.clone()).await;
    if !audit.orphan_voters.is_empty()
        || !audit.duplicate_desired_members.is_empty()
        || !audit.missing_desired_members.is_empty()
        || audit.unexpected_learners != BTreeSet::from([raft_node_id])
    {
        anyhow::bail!(
            "stale learner recovery target must be the unique unexpected learner: \
             orphan_voters={:?}, duplicate_desired_members={:?}, unexpected_learners={:?}, \
             missing_desired_members={:?}",
            audit.orphan_voters,
            audit.duplicate_desired_members,
            audit.unexpected_learners,
            audit.missing_desired_members,
        )
    }
    Ok(input)
}
