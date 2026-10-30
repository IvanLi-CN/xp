use super::*;
use sha2::Sha256;

/// Export deletion intent without loading the ordinary source-history snapshot.
pub(super) async fn page(
    state: &AppState,
    encoded: Option<&str>,
    limit: usize,
) -> anyhow::Result<RepositoryInitialBackfillPage> {
    let cursor = encoded
        .map(HistoricalBackfillPageCursor::decode)
        .transpose()?;
    if cursor.as_ref().is_some_and(|cursor| {
        cursor.snapshot_end_unix_seconds.is_none() || cursor.deletion_snapshot.is_none()
    }) {
        anyhow::bail!("source deletion cursor lacks its snapshot binding");
    }
    let observed_at = cursor
        .as_ref()
        .and_then(|cursor| cursor.snapshot_end_unix_seconds)
        .unwrap_or_else(|| u64::try_from(chrono::Utc::now().timestamp()).unwrap_or_default());
    let expected = cursor
        .as_ref()
        .and_then(|cursor| cursor.deletion_snapshot.clone());
    let mut collector = HistoricalBackfillCollector::new(cursor.map(|cursor| cursor.after), limit)
        .with_snapshot_end(observed_at);
    let mut digest = Sha256::new();
    digest.update(b"xp-source-deletion-snapshot-v1\0");
    let node_id = RepositoryNodeId::try_from(state.cluster.node_id.clone())?;
    let identity = derived_repository_identity(state, node_id)
        .map_err(|_| anyhow::anyhow!("derive source deletion identity"))?;
    let identity_bytes = serde_json::to_vec(&identity)?;
    for field in [
        state.cluster.cluster_id.as_bytes(),
        state.cluster.node_id.as_bytes(),
        identity_bytes.as_slice(),
    ] {
        digest.update((field.len() as u64).to_be_bytes());
        digest.update(field);
    }
    let deletion_version = state
        .node_history
        .visit_repository_deletion_markers(&state.cluster.node_id, true, |marker| {
            let target = marker
                .target_node_id()
                .unwrap_or(&state.cluster.node_id)
                .to_owned();
            for field in [
                marker.schema_id.as_bytes(),
                marker.record_key.as_slice(),
                target.as_bytes(),
            ] {
                digest.update((field.len() as u64).to_be_bytes());
                digest.update(field);
            }
            let tombstone = source_record_with_key_for_subject(
                &marker.schema_id,
                &target,
                &target,
                observed_at,
                marker.record_key,
                serde_json::json!({"deleted_at_unix_seconds": observed_at}),
                true,
            )?;
            collector.push_with_times(0, observed_at, tombstone)
        })
        .await?;
    digest.update(deletion_version.to_be_bytes());
    let snapshot = format!("v2:{deletion_version}:{}", hex::encode(digest.finalize()));
    if expected
        .as_ref()
        .is_some_and(|expected| expected != &snapshot)
    {
        anyhow::bail!("source deletion snapshot changed; restart export");
    }
    collector.deletion_snapshot = Some(snapshot.clone());
    let mut records = Vec::new();
    let mut after = None;
    for (key, (observed, record)) in &collector.records {
        let candidate = RepositoryInitialBackfillRecord {
            observed_at_unix_seconds: *observed,
            source_node_id: None,
            source_epoch: None,
            stream: None,
            sequence: None,
            subject_node_id: record.subject_node_id().to_owned(),
            observer_node_id: record.observer_node_id().to_owned(),
            schema_id: record.schema().0.to_owned(),
            schema_version: record.schema().1,
            record_key_base64: URL_SAFE_NO_PAD.encode(record.record_key()),
            payload_base64: URL_SAFE_NO_PAD.encode(record.payload_bytes()),
            tombstone: true,
        };
        let candidate_bytes = serde_json::to_vec(&candidate)?.len();
        let page_bytes = serde_json::to_vec(&records)?.len();
        if !records.is_empty()
            && page_bytes.saturating_add(candidate_bytes) > MAX_INITIAL_BACKFILL_PAGE_BYTES
        {
            break;
        }
        anyhow::ensure!(
            candidate_bytes <= MAX_INITIAL_BACKFILL_PAGE_BYTES,
            "source deletion record exceeds page budget"
        );
        records.push(candidate);
        after = Some(key.clone());
    }
    let next_page_cursor =
        next_historical_backfill_page_cursor(&collector, records.len(), after.as_ref())?;
    Ok(RepositoryInitialBackfillPage {
        records,
        next_page_cursor,
        deletion_snapshot: Some(snapshot),
    })
}
