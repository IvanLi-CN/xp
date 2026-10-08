use super::{
    AppState, RepositoryLifecycle, RepositoryMemberRuntimePatch, RepositoryMemberRuntimeUpdate,
    RepositoryNodeId,
};

pub(super) async fn update_local_replica_convergence(
    state: &AppState,
    replica_converged: bool,
) -> anyhow::Result<()> {
    let replica_converged =
        replica_converged && !state.repository_replica.lock().await.history_is_truncated();
    let node_id = RepositoryNodeId::try_from(state.cluster.node_id.clone())?;
    let update_needed = {
        let store = state.store.lock().await;
        store
            .state()
            .repository_membership
            .as_ref()
            .and_then(|membership| membership.repository(&node_id))
            .is_some_and(|member| {
                member.lifecycle() == &RepositoryLifecycle::Ready
                    && member.replica_converged() != replica_converged
            })
    };
    if !update_needed {
        return Ok(());
    }
    super::super::super::raft_write(
        state,
        crate::state::DesiredStateCommand::UpdateRepositoryMemberRuntime(
            RepositoryMemberRuntimePatch {
                node_id: node_id.as_str().to_owned(),
                update: RepositoryMemberRuntimeUpdate::ReplicaConverged { replica_converged },
            },
        ),
    )
    .await
    .map_err(|_| anyhow::anyhow!("write local history repository convergence to Raft"))?;
    Ok(())
}
