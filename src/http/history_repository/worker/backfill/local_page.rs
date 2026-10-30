use super::*;

pub(super) async fn resume_local_history_backfill_page(
    state: &AppState,
    identity: &crate::state::history_repository::identity::RepositoryNodeIdentity,
    now_unix_seconds: u64,
    ready_repository_ids: &[String],
    page_cursor: Option<String>,
    completed: bool,
) -> anyhow::Result<super::InitialBackfillProgress> {
    if let Some(cursor) = page_cursor.as_deref() {
        super::HistoricalBackfillPageCursor::decode(cursor)?;
    }
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
        Ok(if completed {
            super::InitialBackfillProgress::Complete
        } else {
            super::InitialBackfillProgress::InProgress
        })
    } else {
        if !super::super::super::identity_is_valid_for_history_replay(state, identity)
            .await
            .map_err(|_| anyhow::anyhow!("check local history backfill segment identity"))?
        {
            anyhow::bail!("local history backfill segment identity is not pinned");
        }
        let outcome = super::legacy_recovery::recover_legacy_original_page(
            state,
            identity,
            now_unix_seconds,
            ready_repository_ids,
        )
        .await?;
        Ok(match outcome {
            super::legacy_recovery::LegacyRecoveryOutcome::Complete if completed => {
                super::InitialBackfillProgress::Complete
            }
            super::legacy_recovery::LegacyRecoveryOutcome::Complete
            | super::legacy_recovery::LegacyRecoveryOutcome::InProgress => {
                super::InitialBackfillProgress::InProgress
            }
            super::legacy_recovery::LegacyRecoveryOutcome::Unavailable => {
                super::InitialBackfillProgress::Unavailable
            }
            super::legacy_recovery::LegacyRecoveryOutcome::NotLegacy => {
                anyhow::bail!("local history backfill page disappeared during recovery")
            }
        })
    }
}
