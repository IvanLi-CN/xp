use super::*;

pub(super) async fn sync_local_repository_capacity(state: &AppState) -> anyhow::Result<()> {
    let node_id = RepositoryNodeId::try_from(state.cluster.node_id.clone())?;
    let configured = {
        let store = state.store.lock().await;
        store
            .state()
            .repository_membership
            .as_ref()
            .is_some_and(|membership| membership.repository(&node_id).is_some())
    };
    // Former repositories retain their archive for recovery, but have no capacity to advertise.
    // Check the role before waiting for the runtime or touching that archive's SQLite connection.
    if !configured {
        return Ok(());
    }
    let mut runtime = state.repository_replica.lock().await;
    let capacity = repository_blocking(|| runtime.runtime_capacity())?;
    drop(runtime);
    let capacity_is_current = {
        let store = state.store.lock().await;
        store
            .state()
            .repository_membership
            .as_ref()
            .and_then(|membership| membership.repository(&node_id))
            .is_none_or(|member| {
                // This Raft patch carries usage, not the node-local admission quota.
                member.capacity().used_bytes() == capacity.used_bytes()
                    && member.capacity().filesystem_available_bytes()
                        == capacity.filesystem_available_bytes()
            })
    };
    if capacity_is_current {
        return Ok(());
    }
    crate::http::raft_write(
        state,
        crate::state::DesiredStateCommand::UpdateRepositoryMemberRuntime(
            RepositoryMemberRuntimePatch {
                node_id: node_id.as_str().to_owned(),
                update: RepositoryMemberRuntimeUpdate::Capacity {
                    used_bytes: capacity.used_bytes(),
                    filesystem_available_bytes: capacity.filesystem_available_bytes(),
                },
            },
        ),
    )
    .await
    .map_err(|_| anyhow::anyhow!("write local history repository capacity to Raft"))?;
    Ok(())
}
