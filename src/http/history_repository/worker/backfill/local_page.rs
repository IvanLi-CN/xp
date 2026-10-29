use super::*;

pub(super) async fn resume_local_history_backfill_page(
    state: &AppState,
    identity: &crate::state::history_repository::identity::RepositoryNodeIdentity,
    now_unix_seconds: u64,
    ready_repository_ids: &[String],
    page_cursor: Option<String>,
    completed: bool,
) -> anyhow::Result<()> {
    let persisted_page = state
        .repository_replica
        .lock()
        .await
        .local_history_backfill_inflight_is_persisted();
    if persisted_page {
        state
            .repository_replica
            .lock()
            .await
            .apply_local_history_backfill_inflight_page(
                &state.cluster.cluster_id,
                identity,
                now_unix_seconds,
                ready_repository_ids,
                &state.cluster.node_id,
            )?;
    } else {
        // Keep the legacy durable page replay path for upgrades that already have a page
        // queued by the predecessor. New pages persist their wire set separately.
        let segments = state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_inflight_segments()?;
        for segment in &segments {
            receive_local_source_segment(
                state,
                segment,
                &[],
                ready_repository_ids,
                now_unix_seconds,
            )
            .await?;
        }
        if !segments.is_empty() {
            state
                .repository_replica
                .lock()
                .await
                .acknowledge_local_source_segments_and_checkpoint_backfill(
                    &segments,
                    page_cursor,
                    completed,
                )?;
        }
    }
    Ok(())
}
