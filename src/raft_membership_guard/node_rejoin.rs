//! Explicit rejoin of a DesiredState node that lost its Raft identity.
//!
//! A single-node quorum recovery shrinks Raft to the surviving voter without deleting any
//! DesiredState record, because the retired nodes may still be serving user traffic. Each of them
//! then needs an explicit next step. Rejoin is the one that keeps the node running: it admits the
//! target through the existing durable `Restore` lifecycle, so the Node, Endpoint and port all
//! survive.
//!
//! This module owns the operator-facing preconditions and the identity-preserving join token.
//! Membership changes stay entirely with the `Restore` lifecycle coordinator.

use std::{collections::BTreeSet, sync::Arc};

use tokio::sync::Mutex;

use crate::{
    raft::{
        app::RaftFacade,
        types::{NodeId, NodeMeta, raft_node_id_from_ulid},
    },
    state::{MembershipOperationKind, MembershipOperationPhase},
};

/// Everything the operator needs to confirm before the rejoin begins, and to run `xp join` after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejoinNodePreview {
    pub node_id: String,
    pub raft_node_id: NodeId,
    pub node_name: String,
    pub api_base_url: String,
    pub expected_membership: String,
    /// Endpoints the rejoin preserves, so the operator can confirm the data plane is untouched.
    pub endpoint_ids: Vec<String>,
}

/// Prove the target is exactly an absent DesiredState Node that a rejoin may adopt.
pub async fn preview_node_rejoin(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<crate::state::JsonSnapshotStore>>,
    node_id: &str,
) -> anyhow::Result<RejoinNodePreview> {
    let raft_node_id = raft_node_id_from_ulid(node_id)?;
    raft.ensure_linearizable().await?;
    let metrics = raft.metrics().borrow().clone();
    validate_node_rejoin(&metrics, &store, node_id, raft_node_id).await?;

    let (node_name, api_base_url, endpoint_ids) = {
        let store = store.lock().await;
        let node = store
            .get_node(node_id)
            .ok_or_else(|| anyhow::anyhow!("rejoin target is not represented by desired state"))?;
        let mut endpoint_ids = store
            .list_endpoints()
            .into_iter()
            .filter(|endpoint| endpoint.node_id == node_id)
            .map(|endpoint| endpoint.endpoint_id)
            .collect::<Vec<_>>();
        endpoint_ids.sort_unstable();
        (node.node_name, node.api_base_url, endpoint_ids)
    };

    Ok(RejoinNodePreview {
        node_id: node_id.to_string(),
        raft_node_id,
        node_name,
        api_base_url,
        expected_membership: super::membership_revision(&metrics)?,
        endpoint_ids,
    })
}

/// The absent DesiredState members a rejoin of `node_id` still has to tolerate.
///
/// The existing `Restore` admission compares the full `missing_desired_members` set against an
/// explicit allowlist, so the caller must name every other absent node that a quorum recovery left
/// behind. They are reported rather than silently excused.
pub async fn absent_peers(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<crate::state::JsonSnapshotStore>>,
    raft_node_id: NodeId,
) -> anyhow::Result<Vec<String>> {
    let audit = super::audit_membership(raft, store).await;
    let mut peers = audit
        .missing_desired_members
        .into_iter()
        .filter(|candidate| *candidate != raft_node_id)
        .map(|candidate| candidate.to_string())
        .collect::<Vec<_>>();
    peers.sort_unstable();
    Ok(peers)
}

async fn validate_node_rejoin(
    metrics: &openraft::RaftMetrics<NodeId, NodeMeta>,
    store: &Arc<Mutex<crate::state::JsonSnapshotStore>>,
    node_id: &str,
    raft_node_id: NodeId,
) -> anyhow::Result<()> {
    if metrics.state != openraft::ServerState::Leader || metrics.current_leader != Some(metrics.id)
    {
        anyhow::bail!("node rejoin must run on the current local leader")
    }
    let membership = metrics.membership_config.membership();
    if membership.get_joint_config().len() > 1 {
        anyhow::bail!("node rejoin is forbidden during joint consensus")
    }
    if metrics.id == raft_node_id {
        anyhow::bail!("node rejoin target is the current leader, which already holds its identity")
    }
    // A rejoin adopts a node that holds no Raft identity. Anything else is already healthy or needs
    // a narrower command: an orphan voter is repaired, a stale learner is retired or restored.
    if membership.get_node(&raft_node_id).is_some() {
        anyhow::bail!("node rejoin target still holds a Raft identity: raft_node_id={raft_node_id}")
    }

    let store = store.lock().await;
    let Some(operation) = store.state().active_membership_operation() else {
        if store.get_node(node_id).is_none() {
            anyhow::bail!("node rejoin target is not represented by desired state")
        }
        // A duplicate DesiredState record sharing this raft identity would make the audit
        // ambiguous, so refuse before creating any operation.
        let duplicate = duplicate_identity_nodes(&store, node_id, raft_node_id);
        if !duplicate.is_empty() {
            anyhow::bail!(
                "node rejoin target shares its raft identity with other desired state \
                 nodes: {duplicate:?}"
            )
        }
        return Ok(());
    };
    // A rejoin is resumable: the same durable Restore for this exact target may already be active.
    if operation.kind == MembershipOperationKind::Restore
        && operation.raft_node_id == raft_node_id
        && operation.node_id.as_deref() == Some(node_id)
        && !operation.phase.is_terminal()
        && operation.phase != MembershipOperationPhase::Blocked
    {
        return Ok(());
    }
    anyhow::bail!("another membership lifecycle operation is active")
}

/// Evidence marker recorded on the durable `Restore` operation so a later reader can tell an
/// operator rejoin from an ordinary absent-node restore.
pub fn rejoin_evidence() -> &'static str {
    "operator-approved rejoin of an absent desired state node"
}

fn duplicate_identity_nodes(
    store: &crate::state::JsonSnapshotStore,
    node_id: &str,
    raft_node_id: NodeId,
) -> BTreeSet<String> {
    store
        .state()
        .nodes
        .keys()
        .filter(|candidate| candidate.as_str() != node_id)
        .filter(|candidate| raft_node_id_from_ulid(candidate).ok() == Some(raft_node_id))
        .cloned()
        .collect()
}
