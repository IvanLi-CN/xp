use std::time::Duration;

use tokio::time::MissedTickBehavior;

use super::super::super::AppState;

// Separate bounded maintenance from the five-minute network replication cycle. Mutation
// triggers cover late rows behind the migration cursor, so continuous writes do not rewind it.
pub(super) fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(5));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            // Initial source backfill is the only path that can close a historical hole. It is
            // write-heavy and uses the same replica mutex; defer derived summary rebuilds until
            // the source baseline is complete so metadata maintenance cannot starve convergence.
            if super::backfill::initial_history_source_backfill_pending(&state).await {
                continue;
            }
            let replica = state.repository_replica.clone();
            let result = tokio::task::spawn_blocking(move || {
                replica
                    .blocking_lock()
                    .advance_sequence_summary_block_rebuild_page()
            })
            .await;
            match result {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => tracing::debug!(%error, "history sequence summary deferred"),
                Err(error) => tracing::warn!(%error, "history sequence summary worker failed"),
            }
            let started = std::time::Instant::now();
            for _ in 0..8 {
                if started.elapsed() >= Duration::from_secs(2) {
                    break;
                }
                let replica = state.repository_replica.clone();
                let result = tokio::task::spawn_blocking(move || {
                    replica.blocking_lock().compact_repository_payload_page()
                })
                .await;
                match result {
                    Ok(Ok(0)) => break,
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => {
                        tracing::warn!(%error, "history payload encoding deferred");
                        break;
                    }
                    Err(error) => {
                        tracing::warn!(%error, "history payload encoding worker failed");
                        break;
                    }
                }
            }
        }
    });
}
