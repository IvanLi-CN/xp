use super::*;
use crate::{
    control_plane_mesh::{MeshPeerTarget, peer_target_from_node},
    domain::{Endpoint, Node},
    state::JsonSnapshotStore,
};

fn map_ready_repository_peers<F>(
    repository_ids: &[String],
    endpoints: &[Endpoint],
    mut get_node: F,
) -> anyhow::Result<Vec<MeshPeerTarget>>
where
    F: FnMut(&str) -> Option<Node>,
{
    repository_ids
        .iter()
        .map(|repository_id| {
            let node = get_node(repository_id).ok_or_else(|| {
                anyhow::anyhow!("ready repository {repository_id} has no node metadata")
            })?;
            Ok(peer_target_from_node(&node, endpoints))
        })
        .collect()
}

pub(in crate::http::history_repository) fn repository_peer_targets(
    store: &JsonSnapshotStore,
    repository_ids: &[String],
    endpoints: &[Endpoint],
) -> anyhow::Result<Vec<MeshPeerTarget>> {
    map_ready_repository_peers(repository_ids, endpoints, |repository_id| {
        store.get_node(repository_id)
    })
}

pub(in crate::http::history_repository) async fn ready_repository_peers(
    state: &AppState,
) -> anyhow::Result<(Vec<String>, Vec<MeshPeerTarget>)> {
    let store = state.store.lock().await;
    let membership = store
        .state()
        .repository_membership
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("history repository membership is not configured"))?;
    let ready_repository_ids = membership
        .ready_members()
        .map(|member| member.node_id().as_str().to_owned())
        .collect::<Vec<_>>();
    if ready_repository_ids.is_empty() {
        anyhow::bail!("no ready history repository is available");
    }
    let endpoints = store.list_endpoints();
    let peers = repository_peer_targets(&store, &ready_repository_ids, &endpoints)?;
    Ok((ready_repository_ids, peers))
}

#[cfg(test)]
mod tests {
    use super::map_ready_repository_peers;

    #[test]
    fn missing_ready_repository_metadata_blocks_peer_targets() {
        let repository_ids = vec!["stale-ready".to_owned()];
        let error = map_ready_repository_peers(&repository_ids, &[], |_| None)
            .expect_err("stale Ready members must not be treated as acknowledged");
        assert!(error.to_string().contains("no node metadata"));
    }
}
