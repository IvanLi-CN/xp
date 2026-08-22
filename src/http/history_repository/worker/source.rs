use crate::history_sync::SyncRecord;
use crate::state::history_repository::{control::RepositoryLifecycle, identity::RepositoryNodeId};

use super::super::AppState;
use super::MAX_SOURCE_PAYLOAD_BYTES;

pub(super) async fn local_repository_lifecycle(
    state: &AppState,
) -> anyhow::Result<RepositoryLifecycle> {
    let store = state.store.lock().await;
    let node_id = RepositoryNodeId::try_from(state.cluster.node_id.clone())?;
    Ok(store
        .state()
        .repository_membership
        .as_ref()
        .and_then(|membership| membership.repository(&node_id))
        .map(|member| *member.lifecycle())
        .unwrap_or(RepositoryLifecycle::Syncing))
}

pub(super) async fn repair_legacy_tombstone_metadata(
    state: &AppState,
    now_unix_seconds: u64,
) -> anyhow::Result<()> {
    state
        .repository_replica
        .lock()
        .await
        .repair_legacy_tombstone_metadata(now_unix_seconds)?;
    Ok(())
}

pub(super) fn should_fanout_tombstone_acknowledgements(lifecycle: RepositoryLifecycle) -> bool {
    lifecycle == RepositoryLifecycle::Ready
}

pub(super) fn source_record(
    schema_id: &str,
    node_id: &str,
    now: u64,
    payload: serde_json::Value,
    tombstone: bool,
) -> anyhow::Result<SyncRecord> {
    source_record_with_key(
        schema_id,
        node_id,
        now,
        format!("node-history:node:{node_id}:current:{schema_id}:{now}").into_bytes(),
        payload,
        tombstone,
    )
}

pub(super) fn source_record_with_key(
    schema_id: &str,
    node_id: &str,
    now: u64,
    record_key: Vec<u8>,
    payload: serde_json::Value,
    tombstone: bool,
) -> anyhow::Result<SyncRecord> {
    source_record_with_key_for_subject(
        schema_id, node_id, node_id, now, record_key, payload, tombstone,
    )
}

pub(super) fn source_record_with_key_for_subject(
    schema_id: &str,
    subject_node_id: &str,
    observer_node_id: &str,
    now: u64,
    record_key: Vec<u8>,
    payload: serde_json::Value,
    tombstone: bool,
) -> anyhow::Result<SyncRecord> {
    let mut payload = serde_json::to_vec(&payload)?;
    if payload.len() > MAX_SOURCE_PAYLOAD_BYTES {
        tracing::warn!(
            schema_id,
            payload_bytes = payload.len(),
            "history source observation exceeded its bounded segment payload"
        );
        payload = serde_json::to_vec(&serde_json::json!({
            "truncated": true,
            "observed_at_unix_seconds": now,
        }))?;
    }
    Ok(SyncRecord::new(
        subject_node_id,
        observer_node_id,
        schema_id,
        1,
        record_key,
        payload,
        tombstone,
    ))
}

pub(super) fn should_attempt_source_relay(transport_failed: bool, target_is_local: bool) -> bool {
    transport_failed && !target_is_local
}

#[cfg(test)]
mod tests {
    use super::{should_attempt_source_relay, source_record, source_record_with_key};

    #[test]
    fn source_relay_requires_a_direct_transport_failure() {
        assert!(!should_attempt_source_relay(false, false));
        assert!(!should_attempt_source_relay(true, true));
        assert!(should_attempt_source_relay(true, false));
    }

    #[test]
    fn source_tombstone_keeps_the_affected_schema_and_marks_the_record() {
        let tombstone = source_record("runtime.v1", "node-a", 100, serde_json::Value::Null, true)
            .expect("tombstone source record");
        assert!(tombstone.is_tombstone());
        assert_eq!(tombstone.schema().0, "runtime.v1");
        assert!(
            !source_record("runtime.v1", "node-a", 100, serde_json::Value::Null, false,)
                .expect("live source record")
                .is_tombstone()
        );
        assert_eq!(
            source_record_with_key(
                "traffic.v1",
                "node-a",
                100,
                b"deleted-history-key".to_vec(),
                serde_json::Value::Null,
                true,
            )
            .expect("stable deletion record")
            .record_key(),
            b"deleted-history-key",
        );
    }
}
