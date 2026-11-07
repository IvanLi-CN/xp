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
            run_maintenance_cycle(&state).await;
        }
    });
}

pub(super) async fn run_maintenance_cycle(state: &AppState) {
    let local_is_repository = {
        let store = state.store.lock().await;
        store
            .state()
            .repository_membership
            .as_ref()
            .is_some_and(|membership| {
                membership
                    .members()
                    .iter()
                    .any(|member| member.identity().node_id().as_str() == state.cluster.node_id)
            })
    };
    if !local_is_repository {
        return;
    }

    // Initial source backfill is the only path that can close a historical hole. It is
    // write-heavy and uses the same replica mutex; defer derived summary rebuilds until
    // the source baseline is complete so metadata maintenance cannot starve convergence.
    if super::backfill::initial_history_source_backfill_pending(state).await {
        return;
    }
    let summary_started = std::time::Instant::now();
    for _ in 0..16 {
        if summary_started.elapsed() >= Duration::from_secs(2) {
            break;
        }
        let replica = state.repository_replica.clone();
        let result = tokio::task::spawn_blocking(move || {
            replica
                .blocking_lock()
                .advance_sequence_summary_block_rebuild_slice(1)
        })
        .await;
        match result {
            Ok(Ok(true)) => break,
            Ok(Ok(false)) => {}
            Ok(Err(error)) => {
                tracing::debug!(%error, "history sequence summary deferred");
                break;
            }
            Err(error) => {
                tracing::warn!(%error, "history sequence summary worker failed");
                break;
            }
        }
        // A queued live delivery must be able to acquire the fair replica mutex before
        // another derived summary block. Never cancel a running SQLite transaction.
        tokio::task::yield_now().await;
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
