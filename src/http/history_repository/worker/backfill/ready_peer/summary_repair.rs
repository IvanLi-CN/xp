use super::{AppState, RepositoryDirectError, SummaryRepairResult};

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
