use std::time::Duration;

use tokio::time::MissedTickBehavior;

use super::super::super::AppState;

const INTERVAL: Duration = Duration::from_secs(5);
const PAGE_SIZE: usize = 128;

pub(super) fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            if let Err(error) = migrate_page(&state).await {
                tracing::debug!(error = %error, "legacy history segment cursor migration skipped");
            }
        }
    });
}

async fn migrate_page(state: &AppState) -> anyhow::Result<()> {
    let replica = state.repository_replica.clone();
    tokio::task::spawn_blocking(move || {
        replica
            .blocking_lock()
            .migrate_legacy_segment_cursor_index_page(PAGE_SIZE)
    })
    .await
    .map_err(|error| anyhow::anyhow!("join legacy segment cursor migration: {error}"))??;
    Ok(())
}
