use super::*;

pub(super) async fn infer_state_applied(
    paths: &StorePaths,
    meta: &PersistedStateMachineMeta,
) -> bool {
    let Some(last_applied) = meta.last_applied else {
        return false;
    };
    read_json::<PersistedWal>(&paths.wal_json)
        .await
        .ok()
        .flatten()
        .and_then(|wal| {
            wal.entries
                .into_iter()
                .find(|entry| entry.log_id == last_applied)
        })
        .is_some_and(|entry| matches!(entry.payload, EntryPayload::Normal(_)))
}
