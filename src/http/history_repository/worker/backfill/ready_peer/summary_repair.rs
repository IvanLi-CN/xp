use super::{
    AppState, MeshPeerTarget, RepositoryDirectError, RepositoryRepairBatch, SummaryRepairResult,
};

const SUMMARY_REPAIR_RETRY_DELAY_SECONDS: u64 = 60;

pub(super) async fn record_summary_repair_attempt(
    state: &AppState,
    peer_node_id: &str,
    now: u64,
) -> anyhow::Result<()> {
    state
        .repository_replica
        .lock()
        .await
        .record_summary_repair_attempt(peer_node_id, now)?;
    Ok(())
}

pub(super) async fn record_summary_repair_result(
    state: &AppState,
    peer_node_id: &str,
    now: u64,
    result: SummaryRepairResult,
    error_code: Option<&str>,
    retry_after: Option<u64>,
) -> anyhow::Result<()> {
    state
        .repository_replica
        .lock()
        .await
        .record_summary_repair_result(
            peer_node_id,
            now,
            result,
            error_code.map(str::to_owned),
            retry_after,
        )?;
    Ok(())
}

pub(super) fn retry_after(now: u64) -> u64 {
    now.saturating_add(SUMMARY_REPAIR_RETRY_DELAY_SECONDS)
}

pub(super) fn classify_summary_repair_error(
    error: &RepositoryDirectError,
) -> (&'static str, SummaryRepairResult) {
    let message = error.to_string();
    if error.is_transport() {
        ("transport_error", SummaryRepairResult::TransportError)
    } else if error.is_repair_response_changed() {
        ("response_changed", SummaryRepairResult::ResponseChanged)
    } else if message.contains("did not advance") {
        ("no_progress", SummaryRepairResult::NoProgress)
    } else {
        ("storage_error", SummaryRepairResult::StorageError)
    }
}

pub(super) async fn record_summary_repair_error(
    state: &AppState,
    peer_node_id: &str,
    now: u64,
    error: &RepositoryDirectError,
) -> anyhow::Result<()> {
    let (error_code, result) = classify_summary_repair_error(error);
    record_summary_repair_result(
        state,
        peer_node_id,
        now,
        result,
        Some(error_code),
        Some(retry_after(now)),
    )
    .await
}

pub(super) fn repair_response_identity(repair: &RepositoryRepairBatch) -> anyhow::Result<String> {
    let legacy = repair.response_id_digest()?;
    if repair.retained_stream_boundaries.is_empty() {
        if repair.response_content_digest.is_some() {
            anyhow::bail!("repository repair response has an unexpected content digest");
        }
        return Ok(legacy);
    }
    let Some(provided_content_digest) = repair.response_content_digest.as_deref() else {
        anyhow::bail!("repository repair response boundary digest is missing");
    };
    let content = repair.response_content_digest()?;
    if provided_content_digest != content {
        anyhow::bail!("repository repair response content digest does not match its content");
    }
    Ok(format!("{legacy}|{content}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boundary_response() -> RepositoryRepairBatch {
        RepositoryRepairBatch {
            segments: Vec::new(),
            unavailable_segment_ids: Vec::new(),
            gaps: Vec::new(),
            history_truncated: true,
            retained_stream_boundaries: serde_json::from_str(
                r#"[{"source_node_id":"node-a","source_epoch":7,"stream":"runtime",
                    "first_sequence":3}]"#,
            )
            .expect("boundary"),
            response_content_digest: None,
            response_id: None,
        }
    }

    #[test]
    fn boundary_response_requires_content_digest() {
        let mut response = boundary_response();
        assert!(repair_response_identity(&response).is_err());
        response.response_content_digest =
            Some(response.response_content_digest().expect("digest"));
        assert!(repair_response_identity(&response).is_ok());
    }
}

pub(super) async fn validate_repair_response(
    state: &AppState,
    peer: &MeshPeerTarget,
    now: u64,
    repair: &RepositoryRepairBatch,
) -> anyhow::Result<String> {
    let legacy = match repair.response_id_digest() {
        Ok(response_id) => response_id,
        Err(error) => {
            super::record_summary_repair_internal_failure(
                state,
                &peer.node_id,
                now,
                &error.to_string(),
            )
            .await?;
            return Err(error.into());
        }
    };
    if repair
        .response_id
        .as_ref()
        .is_some_and(|provided| provided != &legacy)
    {
        record_summary_repair_result(
            state,
            &peer.node_id,
            now,
            SummaryRepairResult::StorageError,
            Some("response_identity_mismatch"),
            Some(retry_after(now)),
        )
        .await?;
        anyhow::bail!("repository repair response identity does not match its content");
    }
    match repair_response_identity(repair) {
        Ok(response_id) => Ok(response_id),
        Err(error) => {
            super::record_summary_repair_internal_failure(
                state,
                &peer.node_id,
                now,
                &error.to_string(),
            )
            .await?;
            Err(error)
        }
    }
}
