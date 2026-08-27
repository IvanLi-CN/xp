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
    membership_operation_gate, membership_revision, require_clean_membership_for_remove_node,
    write_operation,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreachableVoterEvictionPreview {
    pub node_id: String,
    pub raft_node_id: NodeId,
    pub expected_membership: String,
    pub endpoint_ids: Vec<String>,
    pub endpoint_tags: Vec<String>,
}

/// Preview a narrowly scoped operator eviction of a still-mapped voter. This is deliberately
/// separate from orphan repair: the caller must prove and exclude this exact unreachable target
/// from the retained-voter capability barrier before beginning the durable RemoveNode operation.
pub async fn preview_unreachable_voter_eviction(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<crate::state::JsonSnapshotStore>>,
    node_id: &str,
) -> anyhow::Result<UnreachableVoterEvictionPreview> {
    let raft_node_id = raft_node_id_from_ulid(node_id)?;
    require_clean_membership_for_remove_node(raft.clone(), store.clone()).await?;
    let metrics = raft.metrics().borrow().clone();
    validate_unreachable_voter_eviction(&metrics, &store, node_id, raft_node_id, None).await?;
    let (endpoint_ids, endpoint_tags) = endpoint_snapshot_for_node(&store, node_id).await?;
    Ok(UnreachableVoterEvictionPreview {
        node_id: node_id.to_string(),
        raft_node_id,
        expected_membership: membership_revision(&metrics)?,
        endpoint_ids,
        endpoint_tags,
    })
}

/// Begin an explicit eviction after the caller has completed its retained-voter capability check.
/// The normal remove-node resumer owns the subsequent Raft removal, desired-state deletion, and
/// runtime cleanup so uncertain results keep the same recovery contract as an admin deletion.
pub async fn begin_unreachable_voter_eviction(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<crate::state::JsonSnapshotStore>>,
    node_id: &str,
    expected_membership: &str,
    delete_endpoints: bool,
    expected_endpoint_ids: Vec<String>,
) -> anyhow::Result<MembershipOperation> {
    if !delete_endpoints {
        anyhow::bail!("unreachable voter eviction requires endpoint cleanup confirmation")
    }

    let _guard = membership_operation_gate().lock_owned().await;
    let raft_node_id = raft_node_id_from_ulid(node_id)?;
    require_clean_membership_for_remove_node(raft.clone(), store.clone()).await?;
    let metrics = raft.metrics().borrow().clone();
    validate_unreachable_voter_eviction(
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
        anyhow::bail!("node endpoint set changed since unreachable voter eviction preview")
    }

    let operation = MembershipOperation {
        operation_id: uuid::Uuid::new_v4().to_string(),
        kind: MembershipOperationKind::RemoveNode,
        raft_node_id,
        node_id: Some(node_id.to_string()),
        expected_membership: expected_membership.to_string(),
        phase: MembershipOperationPhase::Prepared,
        legacy: false,
        delete_endpoints,
        expected_endpoint_ids,
        expected_endpoint_tags,
        created_at: Utc::now().to_rfc3339(),
        next_retry_at: None,
        terminal_at: None,
        evidence: Some("operator-approved unreachable voter eviction accepted".to_string()),
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

async fn validate_unreachable_voter_eviction(
    metrics: &openraft::RaftMetrics<NodeId, NodeMeta>,
    store: &Arc<Mutex<crate::state::JsonSnapshotStore>>,
    node_id: &str,
    raft_node_id: NodeId,
    expected_membership: Option<&str>,
) -> anyhow::Result<()> {
    if metrics.state != openraft::ServerState::Leader || metrics.current_leader != Some(metrics.id)
    {
        anyhow::bail!("unreachable voter eviction must run on the current local leader")
    }
    let membership = metrics.membership_config.membership();
    if membership.get_joint_config().len() > 1 {
        anyhow::bail!("unreachable voter eviction is forbidden during joint consensus")
    }
    if metrics.id == raft_node_id {
        anyhow::bail!("unreachable voter eviction cannot remove the current leader")
    }
    if !membership
        .voter_ids()
        .any(|voter_id| voter_id == raft_node_id)
    {
        anyhow::bail!("unreachable voter eviction target is not a current voter")
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
    if store.get_node(node_id).is_none() {
        anyhow::bail!("unreachable voter eviction target is not represented by desired state")
    }
    if store
        .state()
        .join_sessions
        .values()
        .filter(|session| session.status.is_pending())
        .filter_map(|session| raft_node_id_from_ulid(&session.node_id).ok())
        .any(|pending_id| pending_id == raft_node_id)
    {
        anyhow::bail!("unreachable voter eviction target has a pending join session")
    }
    Ok(())
}
