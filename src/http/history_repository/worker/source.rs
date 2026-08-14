use crate::history_sync::SyncRecord;

use super::MAX_SOURCE_PAYLOAD_BYTES;

pub(super) fn source_record(
    schema_id: &str,
    node_id: &str,
    now: u64,
    payload: serde_json::Value,
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
        node_id,
        node_id,
        schema_id,
        1,
        format!("{schema_id}:{now}").into_bytes(),
        payload,
        false,
    ))
}

pub(super) fn should_attempt_source_relay(transport_failed: bool, target_is_local: bool) -> bool {
    transport_failed && !target_is_local
}

#[cfg(test)]
mod tests {
    use super::should_attempt_source_relay;

    #[test]
    fn source_relay_requires_a_direct_transport_failure() {
        assert!(!should_attempt_source_relay(false, false));
        assert!(!should_attempt_source_relay(true, true));
        assert!(should_attempt_source_relay(true, false));
    }
}
