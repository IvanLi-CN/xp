use std::time::Duration;

use tokio::time::MissedTickBehavior;

use super::super::super::AppState;

const INTERVAL: Duration = Duration::from_secs(5);

/// Build the record page index outside the startup path.
///
/// The index is what keeps a subject-scoped history page from reading one payload page per
/// candidate row, but on a multi-gigabyte repository the build takes over a minute. Startup
/// preparation cannot own that work: container readiness waits only twenty seconds, so a slow
/// build would be rolled back by the next restart and never commit. Queries remain correct while
/// the index is missing, so the build runs here and startup only checks for it.
pub(super) fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            if let Err(error) = build_once(&state).await {
                tracing::debug!(error = %error, "history record page index build skipped");
                continue;
            }
            // The subject coverage indexes share this worker: both are maintenance work that
            // must not delay container readiness.
            if let Err(error) = build_subject_indexes_once(&state).await {
                tracing::debug!(error = %error, "history subject coverage index build skipped");
                continue;
            }
            // Present or just built: nothing left to do until the next process start.
            break;
        }
    });
}

async fn build_once(state: &AppState) -> anyhow::Result<()> {
    let replica = state.repository_replica.clone();
    let built =
        tokio::task::spawn_blocking(move || replica.blocking_lock().build_repository_page_index())
            .await
            .map_err(|error| anyhow::anyhow!("join history record page index build: {error}"))??;
    if built {
        tracing::info!("history record page index built");
    }
    Ok(())
}

async fn build_subject_indexes_once(state: &AppState) -> anyhow::Result<()> {
    let replica = state.repository_replica.clone();
    let built = tokio::task::spawn_blocking(move || {
        replica.blocking_lock().build_repository_subject_indexes()
    })
    .await
    .map_err(|error| anyhow::anyhow!("join history subject index build: {error}"))??;
    if built {
        tracing::info!("history subject coverage indexes built");
    }
    Ok(())
}
