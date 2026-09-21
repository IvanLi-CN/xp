use super::*;
use crate::{
    control_plane_mesh::{MeshPeerTarget, peer_target_from_node},
    domain::{Endpoint, Node},
};

#[cfg(test)]
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

fn map_ready_repository_peers_for_catch_up<F>(
    repository_ids: &[String],
    endpoints: &[Endpoint],
    mut get_node: F,
) -> (Vec<MeshPeerTarget>, bool)
where
    F: FnMut(&str) -> Option<Node>,
{
    let mut peers = Vec::with_capacity(repository_ids.len());
    let mut missing_metadata = false;
    for repository_id in repository_ids {
        match get_node(repository_id) {
            Some(node) => peers.push(peer_target_from_node(&node, endpoints)),
            None => {
                missing_metadata = true;
                tracing::warn!(
                    repository_id,
                    "ready history repository has no node metadata; catch-up remains incomplete"
                );
            }
        }
    }
    (peers, missing_metadata)
}

pub(in crate::http::history_repository) fn available_ready_repository_ids(
    ready_repository_ids: &[String],
    peers: &[MeshPeerTarget],
) -> Vec<String> {
    ready_repository_ids
        .iter()
        .filter(|repository_id| peers.iter().any(|peer| &peer.node_id == *repository_id))
        .cloned()
        .collect()
}

pub(in crate::http::history_repository) async fn ready_repository_peers(
    state: &AppState,
) -> anyhow::Result<(Vec<String>, Vec<MeshPeerTarget>)> {
    let (ready_repository_ids, peers, _) =
        ready_repository_peers_with_metadata_status(state).await?;
    Ok((ready_repository_ids, peers))
}

pub(in crate::http::history_repository) async fn ready_repository_peers_with_metadata_status(
    state: &AppState,
) -> anyhow::Result<(Vec<String>, Vec<MeshPeerTarget>, bool)> {
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
    let (peers, missing_metadata) = map_ready_repository_peers_for_catch_up(
        &ready_repository_ids,
        &endpoints,
        |repository_id| store.get_node(repository_id),
    );
    Ok((ready_repository_ids, peers, missing_metadata))
}

pub(in crate::http::history_repository) async fn ready_repository_peers_for_catch_up(
    state: &AppState,
) -> anyhow::Result<(Vec<String>, Vec<MeshPeerTarget>, bool)> {
    ready_repository_peers_with_metadata_status(state).await
}

#[cfg(test)]
mod tests {
    use super::{
        available_ready_repository_ids, map_ready_repository_peers,
        map_ready_repository_peers_for_catch_up,
    };

    #[test]
    fn missing_ready_repository_metadata_blocks_peer_targets() {
        let repository_ids = vec!["stale-ready".to_owned()];
        let error = map_ready_repository_peers(&repository_ids, &[], |_| None)
            .expect_err("stale Ready members must not be treated as acknowledged");
        assert!(error.to_string().contains("no node metadata"));
    }

    #[test]
    fn missing_ready_repository_metadata_keeps_catch_up_incomplete_without_blocking_peers() {
        let repository_ids = vec!["stale-ready".to_owned()];
        let (peers, missing_metadata) =
            map_ready_repository_peers_for_catch_up(&repository_ids, &[], |_| None);
        assert!(peers.is_empty());
        assert!(missing_metadata);
    }

    #[test]
    fn stale_ready_repository_is_excluded_from_acknowledgement_targets() {
        let ready_repository_ids = vec!["repo-a".to_owned(), "stale-ready".to_owned()];
        let peers = vec![crate::control_plane_mesh::MeshPeerTarget {
            node_id: "repo-a".to_owned(),
            node_name: "repo-a".to_owned(),
            mesh_base_url: None,
            endpoint_transport: None,
            mesh_reason: crate::mesh_telemetry::MeshPeerReason::MissingEndpoint,
            public_base_url: "https://repo-a.invalid".to_owned(),
        }];

        assert_eq!(
            available_ready_repository_ids(&ready_repository_ids, &peers),
            vec!["repo-a".to_owned()]
        );
    }
}
