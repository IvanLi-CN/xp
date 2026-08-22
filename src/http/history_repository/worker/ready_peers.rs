use super::*;
use crate::control_plane_mesh::peer_target_from_node;

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
    let peers = ready_repository_ids
        .iter()
        .filter_map(|repository_id| store.get_node(repository_id))
        .map(|node| peer_target_from_node(&node, &endpoints))
        .collect();
    Ok((ready_repository_ids, peers))
}
