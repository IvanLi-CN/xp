use axum::http::Method;
use chrono::Utc;
use serde::de::DeserializeOwned;

use crate::{
    control_plane_mesh::{
        MeshPeerTarget, MeshRequest, PeerDirectPath, ReverseRelayRoute, peer_target_from_node,
    },
    history_sync::DirectPath,
    internal_auth::InternalRoute,
};

use super::{AppState, REPOSITORY_REQUEST_BUDGET};

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

pub(crate) fn is_transport_failure(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<RepositoryDirectError>()
        .is_some_and(RepositoryDirectError::is_transport)
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

pub(crate) async fn eligible_mesh_relay_peers(state: &AppState) -> Vec<MeshPeerTarget> {
    all_cluster_peers(state)
        .await
        .into_iter()
        .filter(|peer| peer.mesh_base_url.is_some())
        .collect()
}

/// Dynamic relay is an authenticated Mesh-only transport. It deliberately does not participate
/// in the source-to-repository direct-path selector, whose Reality Mesh and Tunnel paths are
/// equal alternatives and must both fail before this call is reachable.
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
    let now = u64::try_from(Utc::now().timestamp()).unwrap_or_default();
    let (preferred, probe_standby) = {
        let mut runtime = state.repository_replica.lock().await;
        runtime
            .select_peer_direct_path(peer.node_id.as_str(), peer.mesh_base_url.is_some(), now)
            .map_err(|error| RepositoryDirectError::Transport(error.into()))?
    };
    let preferred_path = peer_direct_path(preferred);
    let alternate_path = match preferred {
        DirectPath::RealityMesh => PeerDirectPath::ApiBaseUrl,
        DirectPath::CloudflareTunnel => PeerDirectPath::RealityMesh,
    };
    let mut last_error = None::<RepositoryDirectError>;
    for path in [preferred_path, alternate_path] {
        match send_repository_request_on_path(
            state,
            peer,
            path,
            method.clone(),
            path_and_query,
            body.clone(),
            true,
        )
        .await
        {
            Ok(decoded) => {
                let _ = state
                    .repository_replica
                    .lock()
                    .await
                    .record_peer_direct_path_result(
                        peer.node_id.as_str(),
                        history_direct_path(path),
                        true,
                        now,
                    );
                if path == preferred_path && probe_standby {
                    probe_repository_path(state, peer, alternate_path, now).await;
                }
                return Ok(decoded);
            }
            Err(error) => {
                let healthy_response = !error.is_transport();
                let _ = state
                    .repository_replica
                    .lock()
                    .await
                    .record_peer_direct_path_result(
                        peer.node_id.as_str(),
                        history_direct_path(path),
                        healthy_response,
                        now,
                    );
                if healthy_response {
                    return Err(error);
                }
                last_error = Some(error);
            }
        }
    }
    let direct_error = last_error.expect("both direct paths were attempted");
    if configure_repository_reverse_route(state, peer).await {
        let reverse_request = MeshRequest {
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
            updates_active_path: true,
        };
        if let Ok(response) = state
            .mesh_client
            .send_peer_reverse_request(
                peer,
                reverse_request,
                state.cluster_ca_key_pem.as_deref().ok_or_else(|| {
                    RepositoryDirectError::Transport(anyhow::anyhow!(
                        "cluster CA key is not available"
                    ))
                })?,
                &state.cluster_ca_pem,
            )
            .await
            .map_err(|error| RepositoryDirectError::Transport(error.into()))
        {
            if !response.status().is_success() {
                return Err(RepositoryDirectError::Application(anyhow::anyhow!(
                    "repository reverse peer rejected request with {}",
                    response.status()
                )));
            }
            return response
                .json::<T>()
                .await
                .map_err(|error| RepositoryDirectError::Application(error.into()));
        }
    }
    Err(direct_error)
}

async fn configure_repository_reverse_route(state: &AppState, peer: &MeshPeerTarget) -> bool {
    let route = {
        let store = state.store.lock().await;
        let nodes = store.list_nodes();
        let endpoints = store.list_endpoints();
        (|| {
            let assignment = store
                .state()
                .reverse_mesh_assignments
                .get(&peer.node_id)
                .cloned()?;
            let primary_node = nodes
                .iter()
                .find(|node| node.node_id == assignment.primary_node_id)?;
            let primary = peer_target_from_node(primary_node, &endpoints);
            let standby = assignment.standby_node_id.as_ref().and_then(|standby_id| {
                nodes
                    .iter()
                    .find(|node| node.node_id == *standby_id)
                    .map(|node| peer_target_from_node(node, &endpoints))
            });
            Some(ReverseRelayRoute {
                rendezvous: primary,
                standby_rendezvous: standby,
                assignment,
                role: crate::reverse_mesh::ReverseRole::Primary,
            })
        })()
    };
    match route {
        Some(route) => {
            state
                .mesh_client
                .set_reverse_route(peer.node_id.clone(), route)
                .await;
            true
        }
        None => {
            state.mesh_client.clear_reverse_route(&peer.node_id).await;
            false
        }
    }
}

fn peer_direct_path(path: DirectPath) -> PeerDirectPath {
    match path {
        DirectPath::RealityMesh => PeerDirectPath::RealityMesh,
        DirectPath::CloudflareTunnel => PeerDirectPath::ApiBaseUrl,
    }
}

fn history_direct_path(path: PeerDirectPath) -> DirectPath {
    match path {
        PeerDirectPath::RealityMesh => DirectPath::RealityMesh,
        PeerDirectPath::ApiBaseUrl => DirectPath::CloudflareTunnel,
    }
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

async fn probe_repository_path(
    state: &AppState,
    peer: &MeshPeerTarget,
    path: PeerDirectPath,
    now: u64,
) {
    let result = send_repository_request_on_path::<serde_json::Value>(
        state,
        peer,
        path,
        Method::GET,
        "/api/admin/_internal/history-repository/status",
        Vec::new(),
        false,
    )
    .await;
    let _ = state
        .repository_replica
        .lock()
        .await
        .record_peer_direct_path_result(
            peer.node_id.as_str(),
            history_direct_path(path),
            !matches!(&result, Err(error) if error.is_transport()),
            now,
        );
}

#[cfg(test)]
mod tests {
    use super::{RepositoryDirectError, is_transport_failure};

    #[test]
    fn application_rejection_is_not_a_transport_failure() {
        let rejection = RepositoryDirectError::Application(anyhow::anyhow!("409 conflict"));
        assert!(!rejection.is_transport());
        let rejection = anyhow::Error::new(rejection);
        assert!(!is_transport_failure(&rejection));
        let transport = anyhow::Error::new(RepositoryDirectError::Transport(anyhow::anyhow!(
            "connection reset"
        )));
        assert!(is_transport_failure(&transport));
    }
}
