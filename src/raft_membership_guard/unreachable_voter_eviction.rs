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
    /// Retained voters the operator named as also permanently gone. The leader re-classifies each
    /// one itself before they are excluded from the capability barrier.
    pub confirmed_unreachable: Vec<NodeId>,
}

/// Preview a narrowly scoped operator eviction of a still-mapped voter. This is deliberately
/// separate from orphan repair: the caller must prove and exclude this exact unreachable target
/// from the retained-voter capability barrier before beginning the durable RemoveNode operation.
pub async fn preview_unreachable_voter_eviction(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<crate::state::JsonSnapshotStore>>,
    node_id: &str,
    confirmed_unreachable: &[NodeId],
    confirmed_absent: bool,
) -> anyhow::Result<UnreachableVoterEvictionPreview> {
    let raft_node_id = raft_node_id_from_ulid(node_id)?;
    if confirmed_absent {
        require_clean_membership_except_absent(raft.clone(), store.clone(), &[raft_node_id])
            .await?;
    } else {
        require_clean_membership_for_remove_node(raft.clone(), store.clone()).await?;
    }
    let metrics = raft.metrics().borrow().clone();
    validate_unreachable_voter_eviction(
        &metrics,
        &store,
        node_id,
        raft_node_id,
        None,
        confirmed_unreachable,
        confirmed_absent,
    )
    .await?;
    let (endpoint_ids, endpoint_tags) = endpoint_snapshot_for_node(&store, node_id).await?;
    Ok(UnreachableVoterEvictionPreview {
        node_id: node_id.to_string(),
        raft_node_id,
        expected_membership: membership_revision(&metrics)?,
        endpoint_ids,
        endpoint_tags,
        confirmed_unreachable: confirmed_unreachable.to_vec(),
    })
}

/// Operator confirmations for a removal.
///
/// `confirmed_absent` switches the target shape from a still-mapped voter to a DesiredState node
/// that holds no Raft identity at all; the two modes share every other guard.
#[derive(Debug, Clone)]
pub struct EvictionConfirmation<'a> {
    pub delete_endpoints: bool,
    pub expected_endpoint_ids: Vec<String>,
    pub confirmed_unreachable: &'a [NodeId],
    pub confirmed_absent: bool,
}

/// Begin an explicit eviction after the caller has completed its retained-voter capability check.
/// The normal remove-node resumer owns the subsequent Raft removal, desired-state deletion, and
/// runtime cleanup so uncertain results keep the same recovery contract as an admin deletion.
pub async fn begin_unreachable_voter_eviction(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<crate::state::JsonSnapshotStore>>,
    node_id: &str,
    expected_membership: &str,
    confirmation: EvictionConfirmation<'_>,
) -> anyhow::Result<MembershipOperation> {
    let EvictionConfirmation {
        delete_endpoints,
        expected_endpoint_ids,
        confirmed_unreachable,
        confirmed_absent,
    } = confirmation;
    if !delete_endpoints {
        anyhow::bail!("unreachable voter eviction requires endpoint cleanup confirmation")
    }

    let _guard = membership_operation_gate().lock_owned().await;
    let raft_node_id = raft_node_id_from_ulid(node_id)?;
    if confirmed_absent {
        require_clean_membership_except_absent(raft.clone(), store.clone(), &[raft_node_id])
            .await?;
    } else {
        require_clean_membership_for_remove_node(raft.clone(), store.clone()).await?;
    }
    let metrics = raft.metrics().borrow().clone();
    validate_unreachable_voter_eviction(
        &metrics,
        &store,
        node_id,
        raft_node_id,
        Some(expected_membership),
        confirmed_unreachable,
        confirmed_absent,
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
        remove_learner: false,
        delete_endpoints,
        expected_endpoint_ids,
        expected_endpoint_tags,
        created_at: Utc::now().to_rfc3339(),
        next_retry_at: None,
        terminal_at: None,
        evidence: Some(eviction_evidence(confirmed_unreachable, confirmed_absent)),
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

/// The authorization reason is free-form evidence, so the confirmed set travels there instead of
/// widening the durable operation schema that every older voter must still decode.
fn eviction_evidence(confirmed_unreachable: &[NodeId], confirmed_absent: bool) -> String {
    let mut evidence = if confirmed_absent {
        String::from("operator-confirmed permanent absence of an absent desired state node")
    } else {
        String::from("operator-approved unreachable voter eviction accepted")
    };
    if confirmed_unreachable.is_empty() {
        return evidence;
    }
    let ids = confirmed_unreachable
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    evidence.push_str("; confirmed unreachable retained voters: ");
    evidence.push_str(&ids);
    evidence
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

/// A cluster that shrinks below this many voters has no spare availability for the next failure.
const MINIMUM_VOTERS_AFTER_EVICTION: usize = 3;

/// A clean membership audit with the absent DesiredState set tolerated.
///
/// Removing absent nodes one at a time would otherwise deadlock: every removal requires a clean
/// audit, and each remaining absent node keeps `missing_desired_members` non-empty. Under this
/// explicit mode the absent set is the work queue itself — one operator-confirmed removal per
/// entry — so tolerating it cannot silently discard a node. The target must still be in that set,
/// and every unrelated incident shape (orphan voter, duplicate DesiredState identity, unexpected
/// learner, conflicting operation) still blocks.
pub(crate) async fn require_clean_membership_except_absent(
    raft: Arc<dyn RaftFacade>,
    store: Arc<Mutex<crate::state::JsonSnapshotStore>>,
    absent_targets: &[NodeId],
) -> anyhow::Result<()> {
    raft.ensure_linearizable().await?;
    let mut audit = crate::raft_membership_guard::audit_membership(raft, store).await;
    for target in absent_targets {
        if !audit.missing_desired_members.contains(target) {
            anyhow::bail!(
                "absent-node removal target is not an absent desired state member: \
                 raft_node_id={target}"
            )
        }
    }
    audit.missing_desired_members.clear();
    audit
        .is_clean()
        .then_some(())
        .ok_or_else(|| crate::raft_membership_guard::membership_invariant_error(&audit))
}

fn confirmed_conflicts(target: NodeId, confirmed_unreachable: &[NodeId]) -> bool {
    confirmed_unreachable.contains(&target)
}

async fn validate_unreachable_voter_eviction(
    metrics: &openraft::RaftMetrics<NodeId, NodeMeta>,
    store: &Arc<Mutex<crate::state::JsonSnapshotStore>>,
    node_id: &str,
    raft_node_id: NodeId,
    expected_membership: Option<&str>,
    confirmed_unreachable: &[NodeId],
    confirmed_absent: bool,
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
    let is_voter = membership
        .voter_ids()
        .any(|voter_id| voter_id == raft_node_id);
    let is_member = membership.get_node(&raft_node_id).is_some();
    if confirmed_absent {
        // An absent DesiredState Node holds no Raft identity, so the ordinary "must be a current
        // voter" rule would reject exactly the shape this mode exists to clean up.
        if is_member {
            anyhow::bail!(
                "absent-node removal target still holds a Raft identity: \
                 raft_node_id={raft_node_id}"
            )
        }
    } else {
        if !is_voter {
            anyhow::bail!("unreachable voter eviction target is not a current voter")
        }
        let remaining_voters = membership.voter_ids().count().saturating_sub(1);
        if remaining_voters < MINIMUM_VOTERS_AFTER_EVICTION {
            anyhow::bail!(
                "evicting this voter would leave {remaining_voters} voters; at least \
                 {MINIMUM_VOTERS_AFTER_EVICTION} are required"
            )
        }
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
    if confirmed_conflicts(raft_node_id, confirmed_unreachable) {
        anyhow::bail!("confirmed unreachable voters must exclude the eviction target and leader")
    }
    for confirmed in confirmed_unreachable {
        if !membership.voter_ids().any(|voter| voter == *confirmed) {
            anyhow::bail!("confirmed unreachable voter {confirmed} is not a current voter")
        }
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
