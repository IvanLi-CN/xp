use super::*;

pub(crate) async fn propagate_tombstone_acknowledgements(
    state: &AppState,
    _ready_repository_ids: &[String],
    acknowledgements: Vec<RepositoryTombstoneAcknowledgement>,
) -> anyhow::Result<()> {
    if acknowledgements.is_empty() {
        return Ok(());
    }
    let peers = all_cluster_peers(state).await;
    let body = serde_json::to_vec(&RepositoryTombstoneAcknowledgementRequest { acknowledgements })?;
    let mut first_delivery_error = None::<anyhow::Error>;
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

pub(crate) fn schedule_tombstone_acknowledgement_fanout(
    state: &AppState,
    ready_repository_ids: Vec<String>,
    acknowledgements: Vec<RepositoryTombstoneAcknowledgement>,
) {
    if acknowledgements.is_empty() {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        if let Err(error) =
            propagate_tombstone_acknowledgements(&state, &ready_repository_ids, acknowledgements)
                .await
        {
            tracing::warn!(error = %error, "history tombstone acknowledgement fanout deferred");
        }
    });
}
