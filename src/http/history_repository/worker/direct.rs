use axum::http::Method;
use serde::de::DeserializeOwned;

use crate::{
    control_plane_mesh::{MeshPeerTarget, MeshRequest, PeerDirectPath, peer_target_from_node},
    internal_auth::InternalRoute,
    state::history_repository::replica::RepositoryRuntimeError,
};

use super::{AppState, REPOSITORY_REQUEST_BUDGET};

pub(crate) async fn preserve_history_truncated(
    state: &AppState,
    history_truncated: bool,
) -> Result<(), RepositoryRuntimeError> {
    if history_truncated {
        state
            .repository_replica
            .lock()
            .await
            .mark_history_truncated()?;
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) enum RepositoryDirectError {
    Transport(anyhow::Error),
    Application(anyhow::Error),
}

impl RepositoryDirectError {
    pub(in crate::http::history_repository) fn is_transport(&self) -> bool {
        matches!(self, Self::Transport(_))
    }
}

impl std::fmt::Display for RepositoryDirectError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(error) | Self::Application(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for RepositoryDirectError {}

pub(crate) async fn all_cluster_peers(state: &AppState) -> Vec<MeshPeerTarget> {
    let store = state.store.lock().await;
    let endpoints = store.list_endpoints();
    store
        .list_nodes()
        .into_iter()
        .filter(|node| node.node_id != state.cluster.node_id)
        .map(|node| peer_target_from_node(&node, &endpoints))
        .collect()
}

pub(super) async fn clear_peer_deep_verification(
    state: &AppState,
    peer_repository_id: &str,
) -> anyhow::Result<()> {
    state
        .repository_replica
        .lock()
        .await
        .clear_direct_peer_deep_verification(peer_repository_id)?;
    Ok(())
}

/// Dynamic relay is an authenticated Mesh-only transport. It is kept separate from the
/// source-to-repository history catch-up path, which always uses the peer's public HTTPS origin.
/// The worker never invokes this transport; it remains only for the legacy relay endpoint.
pub(crate) async fn repository_mesh_request<T>(
    state: &AppState,
    peer: &MeshPeerTarget,
    method: Method,
    path_and_query: &str,
    body: Vec<u8>,
) -> Result<T, RepositoryDirectError>
where
    T: DeserializeOwned,
{
    if peer.mesh_base_url.is_none() {
        return Err(RepositoryDirectError::Application(anyhow::anyhow!(
            "relay peer has no eligible Mesh endpoint"
        )));
    }
    send_repository_request_on_path(
        state,
        peer,
        PeerDirectPath::RealityMesh,
        method,
        path_and_query,
        body,
        false,
    )
    .await
}

pub(crate) async fn repository_direct_request<T>(
    state: &AppState,
    peer: &MeshPeerTarget,
    method: Method,
    path_and_query: &str,
    body: Vec<u8>,
) -> Result<T, RepositoryDirectError>
where
    T: DeserializeOwned,
{
    send_repository_request_on_path(
        state,
        peer,
        repository_direct_path(),
        method,
        path_and_query,
        body,
        true,
    )
    .await
}

fn repository_direct_path() -> PeerDirectPath {
    PeerDirectPath::ApiBaseUrl
}

async fn send_repository_request_on_path<T>(
    state: &AppState,
    peer: &MeshPeerTarget,
    path: PeerDirectPath,
    method: Method,
    path_and_query: &str,
    body: Vec<u8>,
    updates_active_path: bool,
) -> Result<T, RepositoryDirectError>
where
    T: DeserializeOwned,
{
    let request = MeshRequest {
        method,
        path_and_query: path_and_query.to_owned(),
        content_type: (!body.is_empty()).then(|| "application/json".to_owned()),
        body,
        total_budget: REPOSITORY_REQUEST_BUDGET,
        allow_ambiguous_fallback: true,
        request_id: crate::id::new_ulid_string(),
        route: InternalRoute::MeshV2,
        cluster_id: state.cluster.cluster_id.clone(),
        sender_id: state.cluster.node_id.clone(),
        updates_active_path,
    };
    let response = state
        .mesh_client
        .send_peer_direct_request(
            peer,
            path,
            request,
            state.cluster_ca_key_pem.as_deref().ok_or_else(|| {
                RepositoryDirectError::Transport(anyhow::anyhow!("cluster CA key is not available"))
            })?,
            &state.cluster_ca_pem,
        )
        .await
        .map_err(|error| RepositoryDirectError::Transport(error.into()))?;
    if !response.status().is_success() {
        return Err(RepositoryDirectError::Application(anyhow::anyhow!(
            "repository peer rejected request with {}",
            response.status()
        )));
    }
    response
        .json::<T>()
        .await
        .map_err(|error| RepositoryDirectError::Application(error.into()))
}

#[cfg(test)]
mod tests {
    use crate::control_plane_mesh::PeerDirectPath;

    #[test]
    fn repository_direct_path_is_public_even_when_mesh_is_configured() {
        assert_eq!(super::repository_direct_path(), PeerDirectPath::ApiBaseUrl);
    }
}
