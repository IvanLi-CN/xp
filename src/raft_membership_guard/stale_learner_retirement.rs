use std::{collections::BTreeSet, sync::Arc};

use chrono::Utc;
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

use super::{
    audit_membership, is_current_local_leader, membership_operation_gate, membership_revision,
    write_operation,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleLearnerRetirementPreview {
    pub node_id: String,
    pub raft_node_id: NodeId,
    pub expected_membership: String,
    pub endpoint_ids: Vec<String>,
    pub endpoint_tags: Vec<String>,
}

/// Preview explicit retirement of one DesiredState-mapped learner whose server is permanently
/// gone. This path never infers intent from the audit and never promotes the learner.
pub async fn preview_stale_learner_retirement(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<crate::state::JsonSnapshotStore>>,
    node_id: &str,
) -> anyhow::Result<StaleLearnerRetirementPreview> {
    let raft_node_id = raft_node_id_from_ulid(node_id)?;
    raft.ensure_linearizable().await?;
    let metrics = raft.metrics().borrow().clone();
    validate_stale_learner_retirement(&raft, &metrics, &store, node_id, raft_node_id, None).await?;
    let (endpoint_ids, endpoint_tags) = endpoint_snapshot_for_node(&store, node_id).await?;
    Ok(StaleLearnerRetirementPreview {
        node_id: node_id.to_string(),
        raft_node_id,
        expected_membership: membership_revision(&metrics)?,
        endpoint_ids,
        endpoint_tags,
    })
}

/// Persist a RemoveNode operation marked for learner retirement after the exact preview has been
/// confirmed. The normal membership resumer then removes the learner and deletes DesiredState.
pub async fn begin_stale_learner_retirement(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<crate::state::JsonSnapshotStore>>,
    node_id: &str,
    expected_membership: &str,
    delete_endpoints: bool,
    expected_endpoint_ids: Vec<String>,
) -> anyhow::Result<MembershipOperation> {
    if !delete_endpoints {
        anyhow::bail!("stale learner retirement requires endpoint cleanup confirmation")
    }

    let _guard = membership_operation_gate().lock_owned().await;
    let raft_node_id = raft_node_id_from_ulid(node_id)?;
    raft.ensure_linearizable().await?;
    let metrics = raft.metrics().borrow().clone();
    validate_stale_learner_retirement(
        &raft,
        &metrics,
        &store,
        node_id,
        raft_node_id,
        Some(expected_membership),
    )
    .await?;
    let (actual_endpoint_ids, expected_endpoint_tags) =
        endpoint_snapshot_for_node(&store, node_id).await?;
    if actual_endpoint_ids.iter().cloned().collect::<BTreeSet<_>>()
        != expected_endpoint_ids.iter().cloned().collect()
    {
        anyhow::bail!("node endpoint set changed since stale learner retirement preview")
    }

    let operation = MembershipOperation {
        operation_id: uuid::Uuid::new_v4().to_string(),
        kind: MembershipOperationKind::RemoveNode,
        raft_node_id,
        node_id: Some(node_id.to_string()),
        expected_membership: expected_membership.to_string(),
        phase: MembershipOperationPhase::Prepared,
        legacy: false,
        remove_learner: true,
        delete_endpoints,
        expected_endpoint_ids,
        expected_endpoint_tags,
        created_at: Utc::now().to_rfc3339(),
        next_retry_at: None,
        terminal_at: None,
        evidence: Some("operator-approved stale learner retirement accepted".to_string()),
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
    Ok(operation)
}

async fn endpoint_snapshot_for_node(
    store: &Arc<Mutex<crate::state::JsonSnapshotStore>>,
    node_id: &str,
) -> anyhow::Result<(Vec<String>, Vec<String>)> {
    let store = store.lock().await;
    if store.get_node(node_id).is_none() {
        anyhow::bail!("node is not represented by desired state")
    }
    let mut endpoints = store
        .list_endpoints()
        .into_iter()
        .filter(|endpoint| endpoint.node_id == node_id)
        .map(|endpoint| (endpoint.endpoint_id, endpoint.tag))
        .collect::<Vec<_>>();
    endpoints.sort();
    Ok(endpoints.into_iter().unzip())
}

async fn validate_stale_learner_retirement(
    raft: &Arc<dyn RaftFacade>,
    metrics: &openraft::RaftMetrics<NodeId, NodeMeta>,
    store: &Arc<Mutex<crate::state::JsonSnapshotStore>>,
    node_id: &str,
    raft_node_id: NodeId,
    expected_membership: Option<&str>,
) -> anyhow::Result<()> {
    if !is_current_local_leader(metrics) {
        anyhow::bail!("stale learner retirement must run on the current local leader")
    }
    let membership = metrics.membership_config.membership();
    if membership.get_joint_config().len() > 1 {
        anyhow::bail!("stale learner retirement is forbidden during joint consensus")
    }
    if metrics.id == raft_node_id {
        anyhow::bail!("stale learner retirement cannot target the current leader")
    }
    if membership
        .voter_ids()
        .any(|voter_id| voter_id == raft_node_id)
    {
        anyhow::bail!("stale learner retirement target is a voter")
    }
    let raft_node = membership.get_node(&raft_node_id).ok_or_else(|| {
        anyhow::anyhow!("stale learner retirement target is absent from membership")
    })?;
    if let Some(expected_membership) = expected_membership
        && membership_revision(metrics)? != expected_membership
    {
        anyhow::bail!("membership revision changed since dry-run")
    }

    {
        let store = store.lock().await;
        if store.state().active_membership_operation().is_some() {
            anyhow::bail!("another membership operation is active")
        }
        let node = store.get_node(node_id).ok_or_else(|| {
            anyhow::anyhow!("stale learner retirement target is not represented by desired state")
        })?;
        if raft_node_id_from_ulid(&node.node_id)? != raft_node_id {
            anyhow::bail!("stale learner retirement target node id does not match Raft id")
        }
        if store
            .state()
            .join_sessions
            .values()
            .any(|session| session.status.is_pending())
        {
            anyhow::bail!("stale learner retirement requires no pending join session")
        }
        if raft_node.name != node.node_name
            || raft_node.api_base_url != node.api_base_url
            || raft_node.raft_endpoint != node.api_base_url
        {
            anyhow::bail!("stale learner retirement target metadata does not match desired state")
        }
    }

    let audit = audit_membership(raft.clone(), store.clone()).await;
    if !audit.orphan_voters.is_empty()
        || !audit.duplicate_desired_members.is_empty()
        || !audit.missing_desired_members.is_empty()
        || audit.unexpected_learners != BTreeSet::from([raft_node_id])
    {
        anyhow::bail!(
            "stale learner retirement target must be the unique unexpected learner: \
             orphan_voters={:?}, duplicate_desired_members={:?}, unexpected_learners={:?}, \
             missing_desired_members={:?}",
            audit.orphan_voters,
            audit.duplicate_desired_members,
            audit.unexpected_learners,
            audit.missing_desired_members,
        )
    }
    Ok(())
}
