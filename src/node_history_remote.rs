use anyhow::Context;

use crate::{
    control_plane_mesh::{MeshAwareHttpClient, MeshPeerTarget, MeshRequest},
    internal_auth::InternalRoute,
    node_history::NodeHistorySnapshot,
};

pub(super) struct RemoteSyncAuth<'a> {
    cluster_id: &'a str,
    sender_id: &'a str,
    cluster_ca_key_pem: &'a str,
    cluster_ca_pem: &'a str,
}

impl<'a> RemoteSyncAuth<'a> {
    pub(super) fn new(
        cluster_id: &'a str,
        sender_id: &'a str,
        cluster_ca_key_pem: &'a str,
        cluster_ca_pem: &'a str,
    ) -> Self {
        Self {
            cluster_id,
            sender_id,
            cluster_ca_key_pem,
            cluster_ca_pem,
        }
    }
}

async fn send_signed_remote_request(
    auth: &RemoteSyncAuth<'_>,
    client: &MeshAwareHttpClient,
    peer: &MeshPeerTarget,
    method: reqwest::Method,
    path_and_query: String,
    allow_ambiguous_fallback: bool,
) -> anyhow::Result<reqwest::Response> {
    client
        .send_peer_request(
            peer,
            MeshRequest {
                method,
                path_and_query,
                content_type: None,
                body: Vec::new(),
                total_budget: std::time::Duration::from_secs(10),
                allow_ambiguous_fallback,
                request_id: crate::id::new_ulid_string(),
                route: InternalRoute::MeshV2,
                cluster_id: auth.cluster_id.to_string(),
                sender_id: auth.sender_id.to_string(),
                updates_active_path: true,
            },
            auth.cluster_ca_key_pem,
            auth.cluster_ca_pem,
        )
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))
}

pub(super) async fn clear_node_history(
    client: &MeshAwareHttpClient,
    auth: &RemoteSyncAuth<'_>,
    peer: &MeshPeerTarget,
    node_id: &str,
) -> anyhow::Result<()> {
    let response = send_signed_remote_request(
        auth,
        client,
        peer,
        reqwest::Method::DELETE,
        format!("/api/admin/_internal/nodes/{node_id}/history"),
        false,
    )
    .await?;
    if !response.status().is_success() {
        anyhow::bail!("node history cleanup request failed: {}", response.status());
    }
    Ok(())
}

pub(super) async fn clear_user_traffic(
    client: &MeshAwareHttpClient,
    auth: &RemoteSyncAuth<'_>,
    peer: &MeshPeerTarget,
    user_id: &str,
) -> anyhow::Result<()> {
    let response = send_signed_remote_request(
        auth,
        client,
        peer,
        reqwest::Method::DELETE,
        format!("/api/admin/_internal/users/{user_id}/traffic/local"),
        false,
    )
    .await?;
    if !response.status().is_success() {
        anyhow::bail!("user traffic cleanup request failed: {}", response.status());
    }
    Ok(())
}

pub(super) async fn fetch_snapshot(
    client: &MeshAwareHttpClient,
    auth: &RemoteSyncAuth<'_>,
    peer: &MeshPeerTarget,
) -> anyhow::Result<NodeHistorySnapshot> {
    let response = send_signed_remote_request(
        auth,
        client,
        peer,
        reqwest::Method::GET,
        "/api/admin/_internal/nodes/history/local".to_string(),
        true,
    )
    .await?;
    if !response.status().is_success() {
        anyhow::bail!("node history request failed: {}", response.status());
    }
    response
        .json::<NodeHistorySnapshot>()
        .await
        .context("decode node history response")
}
