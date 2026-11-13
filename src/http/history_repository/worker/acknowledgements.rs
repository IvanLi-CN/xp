use super::*;

pub(super) async fn propagate_tombstone_acknowledgements(
    state: &AppState,
    peers: &[MeshPeerTarget],
    ready_repository_ids: &[String],
    acknowledgements: Vec<RepositoryTombstoneAcknowledgement>,
) -> anyhow::Result<()> {
    if acknowledgements.is_empty() {
        return Ok(());
    }
    let body = serde_json::to_vec(&RepositoryTombstoneAcknowledgementRequest { acknowledgements })?;
    let mut first_delivery_error =
        missing_tombstone_ack_peer(ready_repository_ids, &state.cluster.node_id, peers).map(
            |repository_id| anyhow::anyhow!("ready repository {repository_id} has no usable peer"),
        );
    for peer in peers
        .iter()
        .filter(|peer| peer.node_id != state.cluster.node_id)
    {
        if let Err(error) = repository_direct_request::<serde_json::Value>(
            state,
            peer,
            Method::POST,
            "/api/admin/_internal/history-repository/tombstone-ack",
            body.clone(),
        )
        .await
        {
            first_delivery_error.get_or_insert(error.into());
        }
    }
    first_delivery_error.map_or(Ok(()), Err)
}

pub(super) fn missing_tombstone_ack_peer(
    ready_repository_ids: &[String],
    local_repository_id: &str,
    peers: &[MeshPeerTarget],
) -> Option<String> {
    ready_repository_ids
        .iter()
        .filter(|repository_id| repository_id.as_str() != local_repository_id)
        .find(|repository_id| !peers.iter().any(|peer| &peer.node_id == *repository_id))
        .cloned()
}
