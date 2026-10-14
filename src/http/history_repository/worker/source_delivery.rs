use super::super::RepositorySyncRequest;
use super::*;
use crate::state::history_repository::replica::RepositorySyncReceipt;

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
        let body = serde_json::to_vec(&super::super::batch::RepositorySyncBatchRequest {
            requests: requests.clone(),
        })?;
        match repository_direct_request::<super::super::batch::RepositorySyncBatchResponse>(
            state,
            peer,
            Method::POST,
            "/api/admin/_internal/history-repository/sync-batch",
            body,
        )
        .await
        {
            Ok(response) if response.receipts.len() == batch_segments.len() => {
                result.attempted_segments = result
                    .attempted_segments
                    .saturating_add(u64::try_from(batch_segments.len()).unwrap_or(u64::MAX));
                for receipt in response.receipts {
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
                let body = serde_json::to_vec(&RepositorySyncRequest::with_wire(
                    segment.identity.clone(),
                    &segment.wire,
                    segment_gaps.to_vec(),
                )?)?;
                match repository_direct_request::<RepositorySyncReceipt>(
                    state,
                    peer,
                    Method::POST,
                    "/api/admin/_internal/history-repository/sync",
                    body,
                )
                .await
                {
                    Ok(receipt) => {
                        result
                            .tombstone_acknowledgements
                            .extend(receipt.tombstone_acknowledgements().iter().cloned());
                        result.delivered_segments.push(segment.clone());
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
        RepositoryDirectError::Application(_) | RepositoryDirectError::Unsupported(_) => {
            "response_error"
        }
    }
}
