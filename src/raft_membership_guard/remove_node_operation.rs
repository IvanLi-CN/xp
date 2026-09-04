use std::{collections::BTreeSet, sync::Arc};

use tokio::sync::Mutex;

use crate::{
    raft::app::RaftFacade,
    state::{DesiredStateCommand, MembershipOperation, MembershipOperationPhase},
};

use super::{
    block_membership_operation, is_current_local_leader, require_expected_membership,
    transition_operation,
};

pub(super) async fn resume_remove_node_operation(
    raft: &Arc<dyn RaftFacade>,
    store: &Arc<Mutex<crate::state::JsonSnapshotStore>>,
    operation: MembershipOperation,
) -> anyhow::Result<()> {
    let metrics = raft.metrics().borrow().clone();
    if !is_current_local_leader(&metrics) {
        return Ok(());
    }
    let node_id = operation
        .node_id
        .clone()
        .ok_or_else(|| anyhow::anyhow!("remove-node operation is missing node_id"))?;

    match operation.phase {
        MembershipOperationPhase::Prepared => {
            let membership = metrics.membership_config.membership();
            let is_voter = membership
                .voter_ids()
                .any(|member| member == operation.raft_node_id);
            let is_member = membership.get_node(&operation.raft_node_id).is_some();
            if metrics.id == operation.raft_node_id {
                block_membership_operation(raft, operation, "target became leader").await?;
                return Ok(());
            }
            if !is_member {
                let absent_evidence = if operation.remove_learner {
                    "target absent after uncertain RemoveNodes request"
                } else {
                    "target absent after uncertain RemoveVoters request"
                };
                let _ = transition_operation(
                    raft,
                    operation,
                    MembershipOperationPhase::MembershipRemoved,
                    absent_evidence,
                )
                .await?;
                return Ok(());
            }
            if operation.remove_learner {
                if is_voter {
                    block_membership_operation(
                        raft,
                        operation,
                        "stale learner retirement target unexpectedly became voter",
                    )
                    .await?;
                    return Ok(());
                }
                if let Err(error) = require_expected_membership(raft, &operation).await {
                    block_membership_operation(raft, operation, error.to_string()).await?;
                    return Ok(());
                }
                raft.change_membership(
                    openraft::ChangeMembers::RemoveNodes(BTreeSet::from([operation.raft_node_id])),
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
                    anyhow::bail!("stale learner retirement did not make target absent")
                }
                let _ = transition_operation(
                    raft,
                    operation,
                    MembershipOperationPhase::MembershipRemoved,
                    "stale learner removed with RemoveNodes retain=false",
                )
                .await?;
                return Ok(());
            }
            if !is_voter {
                block_membership_operation(
                    raft,
                    operation,
                    "delete target is learner; remove-voters cannot safely make it absent",
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
                anyhow::bail!("remove-node operation did not make target absent")
            }
            let _ = transition_operation(
                raft,
                operation,
                MembershipOperationPhase::MembershipRemoved,
                "target made absent from Raft membership",
            )
            .await?;
        }
        MembershipOperationPhase::MembershipRemoved => {
            if store.lock().await.get_node(&node_id).is_none() {
                return Ok(());
            }
            if let Err(error) = require_expected_membership(raft, &operation).await {
                block_membership_operation(raft, operation, error.to_string()).await?;
                return Ok(());
            }
            let out = raft
                .client_write(DesiredStateCommand::DeleteNode {
                    node_id,
                    delete_endpoints: operation.delete_endpoints,
                    expected_endpoint_ids: operation.expected_endpoint_ids.clone(),
                    join_session: None,
                })
                .await?;
            match out {
                crate::raft::types::ClientResponse::Ok {
                    result: crate::state::DesiredStateApplyResult::NodeDeleted { .. },
                } => {}
                crate::raft::types::ClientResponse::Ok { .. } => {
                    anyhow::bail!("remove-node operation received unexpected desired-state result")
                }
                crate::raft::types::ClientResponse::Err { code, message, .. } => {
                    block_membership_operation(
                        raft,
                        operation,
                        format!("remove-node desired-state delete rejected: {code}: {message}"),
                    )
                    .await?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
