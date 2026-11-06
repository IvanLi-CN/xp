use super::super::RepositorySyncRequest;
use super::*;
use crate::state::history_repository::replica::RepositorySyncReceipt;

pub(super) async fn complete_acked_source_deletions(
    state: &AppState,
    markers: &[crate::node_history::RepositoryHistoryDeletionMarker],
) -> anyhow::Result<()> {
    if markers.is_empty() {
        return Ok(());
    }
    let mut runtime = state.repository_replica.lock().await;
    repository_blocking(|| runtime.complete_local_source_tombstones(markers))?;
    drop(runtime);
    for marker in markers {
        state
            .node_history
            .complete_repository_deletion_marker(&state.cluster.node_id, marker)
            .await?;
    }
    Ok(())
}

pub(super) fn avoid_local_source_fallback(
    selected_repository_id: String,
    primary_repository_id: &str,
    local_repository_id: &str,
    local_repository_ready: bool,
) -> String {
    if !local_repository_ready
        && selected_repository_id == local_repository_id
        && primary_repository_id != local_repository_id
    {
        primary_repository_id.to_owned()
    } else {
        selected_repository_id
    }
}

pub(super) struct RemoteSourceDelivery {
    pub(super) attempted_segments: u64,
    pub(super) delivery_succeeded: bool,
    pub(super) delivery_outcome: &'static str,
    pub(super) delivered_segments:
        Vec<crate::state::history_repository::replica::RepositoryReplicaSegment>,
    pub(super) tombstone_acknowledgements: Vec<RepositoryTombstoneAcknowledgement>,
}

pub(super) async fn deliver_remote_source_segments(
    state: &AppState,
    peer: &MeshPeerTarget,
    selected_repository_id: &str,
    segments: &[crate::state::history_repository::replica::RepositoryReplicaSegment],
    gaps: &[crate::state::history_repository::replica::RepositoryReplicaGap],
) -> anyhow::Result<RemoteSourceDelivery> {
    deliver_with_transport(
        selected_repository_id,
        segments,
        gaps,
        |requests, batch| async move {
            if batch {
                let body = serde_json::to_vec(&super::super::batch::RepositorySyncBatchRequest {
                    requests,
                })
                .map_err(|error| RepositoryDirectError::Application(error.into()))?;
                repository_direct_request::<super::super::batch::RepositorySyncBatchResponse>(
                    state,
                    peer,
                    Method::POST,
                    "/api/admin/_internal/history-repository/sync-batch",
                    body,
                )
                .await
                .map(|response| response.receipts)
            } else {
                let body = serde_json::to_vec(&requests[0])
                    .map_err(|error| RepositoryDirectError::Application(error.into()))?;
                repository_direct_request::<RepositorySyncReceipt>(
                    state,
                    peer,
                    Method::POST,
                    "/api/admin/_internal/history-repository/sync",
                    body,
                )
                .await
                .map(|receipt| vec![receipt])
            }
        },
    )
    .await
}

pub(super) async fn deliver_with_transport<F, Fut>(
    selected_repository_id: &str,
    segments: &[crate::state::history_repository::replica::RepositoryReplicaSegment],
    gaps: &[crate::state::history_repository::replica::RepositoryReplicaGap],
    mut send: F,
) -> anyhow::Result<RemoteSourceDelivery>
where
    F: FnMut(Vec<RepositorySyncRequest>, bool) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<RepositorySyncReceipt>, RepositoryDirectError>>,
{
    let mut batches = Vec::new();
    let mut batch_requests = Vec::new();
    let mut batch_segments = Vec::new();
    let mut batch_wire_bytes = 0_usize;
    for (index, segment) in segments.iter().enumerate() {
        let segment_gaps = if index == 0 { gaps } else { &[] };
        let request = RepositorySyncRequest::with_wire(
            segment.identity.clone(),
            &segment.wire,
            segment_gaps.to_vec(),
        )?;
        let starts_new_batch = !batch_requests.is_empty()
            && (batch_requests.len() >= super::super::batch::MAX_HISTORY_SYNC_BATCH_SEGMENTS
                || batch_wire_bytes.saturating_add(segment.wire.len())
                    > super::super::batch::MAX_HISTORY_SYNC_BATCH_WIRE_BYTES);
        if starts_new_batch {
            batches.push((batch_requests, batch_segments));
            batch_requests = Vec::new();
            batch_segments = Vec::new();
            batch_wire_bytes = 0;
        }
        batch_wire_bytes = batch_wire_bytes.saturating_add(segment.wire.len());
        batch_requests.push(request);
        batch_segments.push(segment.clone());
    }
    if !batch_requests.is_empty() {
        batches.push((batch_requests, batch_segments));
    }

    let mut result = RemoteSourceDelivery {
        attempted_segments: 0,
        delivery_succeeded: true,
        delivery_outcome: "success",
        delivered_segments: Vec::new(),
        tombstone_acknowledgements: Vec::new(),
    };
    let mut fallback_batch_index = None;
    for (batch_index, (requests, batch_segments)) in batches.iter().enumerate() {
        match send(requests.clone(), true).await {
            Ok(receipts) if receipts.len() == batch_segments.len() => {
                result.attempted_segments = result
                    .attempted_segments
                    .saturating_add(u64::try_from(batch_segments.len()).unwrap_or(u64::MAX));
                for receipt in receipts {
                    result
                        .tombstone_acknowledgements
                        .extend(receipt.tombstone_acknowledgements().iter().cloned());
                }
                result
                    .delivered_segments
                    .extend(batch_segments.iter().cloned());
            }
            Ok(_) => {
                result.attempted_segments = result
                    .attempted_segments
                    .saturating_add(u64::try_from(batch_segments.len()).unwrap_or(u64::MAX));
                result.delivery_succeeded = false;
                result.delivery_outcome = "response_error";
                break;
            }
            Err(RepositoryDirectError::Unsupported(_)) => {
                fallback_batch_index = Some(batch_index);
                break;
            }
            Err(RepositoryDirectError::SequenceRejected(_)) if gaps.is_empty() => {
                result.attempted_segments = result
                    .attempted_segments
                    .saturating_add(batch_segments.len() as u64);
                result.delivery_succeeded = false;
                result.delivery_outcome = "response_error";
                recover_independent_streams(requests, batch_segments, &mut result, &mut send)
                    .await?;
                // Keep the original bounded window. Only real receipts authorize its next ACK.
                break;
            }
            Err(error) => {
                result.attempted_segments = result
                    .attempted_segments
                    .saturating_add(u64::try_from(batch_segments.len()).unwrap_or(u64::MAX));
                result.delivery_succeeded = false;
                result.delivery_outcome = source_delivery_error_outcome(&error);
                tracing::debug!(
                    repository = selected_repository_id,
                    error = %error,
                    "history source batch remains queued for retry"
                );
                break;
            }
        }
    }

    if let Some(start) = fallback_batch_index {
        for (batch_index, (_, batch_segments)) in batches.iter().enumerate().skip(start) {
            for (segment_index, segment) in batch_segments.iter().enumerate() {
                result.attempted_segments = result.attempted_segments.saturating_add(1);
                let segment_gaps = if batch_index == 0 && segment_index == 0 {
                    gaps
                } else {
                    &[]
                };
                let request = RepositorySyncRequest::with_wire(
                    segment.identity.clone(),
                    &segment.wire,
                    segment_gaps.to_vec(),
                )?;
                match send(vec![request], false).await {
                    Ok(mut receipts) if receipts.len() == 1 => {
                        let receipt = receipts.remove(0);
                        result
                            .tombstone_acknowledgements
                            .extend(receipt.tombstone_acknowledgements().iter().cloned());
                        result.delivered_segments.push(segment.clone());
                    }
                    Ok(_) => {
                        result.delivery_succeeded = false;
                        result.delivery_outcome = "response_error";
                        break;
                    }
                    Err(error) => {
                        result.delivery_succeeded = false;
                        result.delivery_outcome = source_delivery_error_outcome(&error);
                        tracing::debug!(
                            repository = selected_repository_id,
                            error = %error,
                            "history source segment remains queued for retry"
                        );
                        break;
                    }
                }
            }
            if !result.delivery_succeeded {
                break;
            }
        }
    }
    Ok(result)
}

fn source_delivery_error_outcome(error: &RepositoryDirectError) -> &'static str {
    match error {
        RepositoryDirectError::Transport(_) => "transport_error",
        RepositoryDirectError::RepairResponseChanged => "response_changed",
        RepositoryDirectError::Application(_)
        | RepositoryDirectError::SequenceRejected(_)
        | RepositoryDirectError::Unsupported(_) => "response_error",
    }
}

async fn recover_independent_streams<F, Fut>(
    requests: &[RepositorySyncRequest],
    segments: &[crate::state::history_repository::replica::RepositoryReplicaSegment],
    result: &mut RemoteSourceDelivery,
    send: &mut F,
) -> anyhow::Result<()>
where
    F: FnMut(Vec<RepositorySyncRequest>, bool) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<RepositorySyncReceipt>, RepositoryDirectError>>,
{
    let mut groups = std::collections::BTreeMap::<_, Vec<usize>>::new();
    for (index, segment) in segments.iter().enumerate() {
        let signed = SignedSegment::from_wire(&segment.wire)?;
        let cursor = signed.canonical().first_cursor();
        groups
            .entry((
                cursor.stream() != "tombstone",
                cursor.source_node_id().to_owned(),
                cursor.source_epoch(),
                cursor.stream().to_owned(),
            ))
            .or_default()
            .push(index);
    }
    if groups.len() <= 1 {
        return Ok(());
    }
    // A rejected mixed page must not pin unrelated streams forever. Bound the recovery work,
    // preserve each original stream's order, and finish tombstones before trying live data.
    // Transport ambiguity, authentication and storage failures never authorize this recovery.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    for ((live, _, _, _), indices) in groups.into_iter().take(8) {
        let page = indices
            .iter()
            .map(|&index| requests[index].clone())
            .collect();
        result.attempted_segments = result
            .attempted_segments
            .saturating_add(indices.len() as u64);
        match tokio::time::timeout_at(deadline, send(page, true)).await {
            Ok(Ok(receipts)) if receipts.len() == indices.len() => {
                for receipt in receipts {
                    result
                        .tombstone_acknowledgements
                        .extend(receipt.tombstone_acknowledgements().iter().cloned());
                }
                result
                    .delivered_segments
                    .extend(indices.iter().map(|&index| segments[index].clone()));
            }
            Ok(Err(RepositoryDirectError::SequenceRejected(_))) if live => {}
            _ => break,
        }
    }
    Ok(())
}
