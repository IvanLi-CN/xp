use super::*;
use crate::control_plane_mesh::peer_target_from_node;
use crate::history_sync::ProtocolError;
use crate::http::history_repository::{
    derived_repository_identity, derived_repository_signing_key,
};
use crate::state::history_repository::{
    MAX_INITIAL_BACKFILL_PAGE_BYTES, MAX_INITIAL_BACKFILL_PAGE_RECORDS,
};
mod cursor;
mod historical_source;
mod legacy_recovery;
mod local_page;
mod mesh;
mod ordering;
mod ready_peer;
mod source_deletions;
#[cfg(test)]
mod tests;
mod validation;

use cursor::HistoricalBackfillPageCursor;
pub(crate) use cursor::HistoricalBackfillSortKey;
use historical_source::historical_source_backfill_records;
pub(crate) use ordering::advance_initial_catch_up;
#[cfg(test)]
pub(crate) use ordering::{catch_up_before_pending_local, should_run_initial_catch_up};
pub(crate) use ready_peer::catch_up_against_ready_repositories;
use validation::{deserialize_initial_backfill_page, validate_peer_backfill_page};

// Base64 fields and the JSON envelope expand the canonical page on the wire. Keep the
// pre-decode allocation bounded to four times the semantic page budget; typed validation below
// still enforces the exact record and canonical-byte limits.
const MAX_INITIAL_BACKFILL_WIRE_BYTES: usize = MAX_INITIAL_BACKFILL_PAGE_BYTES * 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InitialBackfillProgress {
    InProgress,
    Complete,
    Unavailable,
}

impl InitialBackfillProgress {
    fn combine(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unavailable, _) | (_, Self::Unavailable) => Self::Unavailable,
            (Self::InProgress, _) | (_, Self::InProgress) => Self::InProgress,
            (Self::Complete, Self::Complete) => Self::Complete,
        }
    }
}
pub(super) struct PeerBackfillImport<'a> {
    identity: &'a crate::state::history_repository::identity::RepositoryNodeIdentity,
    signing_key: &'a ed25519_dalek::SigningKey,
    peer_node_id: &'a str,
    epoch: u64,
    ready_repository_ids: &'a [String],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RepositoryInitialBackfillRecord {
    observed_at_unix_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_epoch: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stream: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sequence: Option<u64>,
    subject_node_id: String,
    observer_node_id: String,
    schema_id: String,
    schema_version: u32,
    record_key_base64: String,
    payload_base64: String,
    tombstone: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RepositoryInitialBackfillPage {
    records: Vec<RepositoryInitialBackfillRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_page_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    deletion_snapshot: Option<String>,
}

pub(crate) struct HistoricalBackfillCollector {
    pub(crate) after: Option<HistoricalBackfillSortKey>,
    snapshot_end_unix_seconds: Option<u64>,
    deletion_snapshot: Option<String>,
    pub(crate) limit: usize,
    pub(crate) records: BTreeMap<HistoricalBackfillSortKey, (u64, SyncRecord)>,
    pub(crate) has_more: bool,
}
impl HistoricalBackfillCollector {
    pub(crate) fn new(after: Option<HistoricalBackfillSortKey>, limit: usize) -> Self {
        Self {
            after,
            snapshot_end_unix_seconds: None,
            deletion_snapshot: None,
            limit,
            records: BTreeMap::new(),
            has_more: false,
        }
    }

    fn with_snapshot_end(mut self, snapshot_end_unix_seconds: u64) -> Self {
        self.snapshot_end_unix_seconds = Some(snapshot_end_unix_seconds);
        self
    }
    pub(crate) fn push(&mut self, record: (u64, SyncRecord)) -> anyhow::Result<()> {
        self.push_with_times(record.0, record.0, record.1)
    }
    pub(crate) fn push_with_times(
        &mut self,
        sort_at_unix_seconds: u64,
        segment_at_unix_seconds: u64,
        record: SyncRecord,
    ) -> anyhow::Result<()> {
        if !record.is_tombstone()
            && self
                .snapshot_end_unix_seconds
                .is_some_and(|end| segment_at_unix_seconds > end)
        {
            return Ok(());
        }
        let key = HistoricalBackfillSortKey {
            observed_at_unix_seconds: sort_at_unix_seconds,
            schema_id: record.schema().0.to_owned(),
            record_key: record.record_key().to_vec(),
        };
        if self.after.as_ref().is_some_and(|after| key <= *after) {
            return Ok(());
        }
        self.records.insert(key, (segment_at_unix_seconds, record));
        if self.records.len() > self.limit {
            self.records.pop_last();
            self.has_more = true;
        }
        while self.records.len() > 1
            && self.serialized_records_bytes()? > MAX_INITIAL_BACKFILL_PAGE_BYTES
        {
            self.records.pop_last();
            self.has_more = true;
        }
        if serialized_backfill_record_bytes(
            &self
                .records
                .values()
                .last()
                .expect("backfill collector requires a record")
                .1,
        )? > MAX_INITIAL_BACKFILL_PAGE_BYTES
        {
            anyhow::bail!("initial history backfill record exceeds page budget");
        }
        Ok(())
    }

    fn serialized_records_bytes(&self) -> anyhow::Result<usize> {
        self.records
            .values()
            .map(|(_, record)| serialized_backfill_record_bytes(record))
            .sum()
    }
    pub(crate) fn next_cursor(&self) -> anyhow::Result<Option<String>> {
        self.has_more
            .then(|| {
                self.encode_cursor_after(
                    self.records
                        .last_key_value()
                        .expect("backfill cursor requires a record")
                        .0,
                )
            })
            .transpose()
    }

    fn encode_cursor_after(&self, after: &HistoricalBackfillSortKey) -> anyhow::Result<String> {
        match self.snapshot_end_unix_seconds {
            Some(snapshot_end_unix_seconds) => HistoricalBackfillPageCursor {
                after: after.clone(),
                snapshot_end_unix_seconds: Some(snapshot_end_unix_seconds),
                deletion_snapshot: self.deletion_snapshot.clone(),
            }
            .encode(),
            None => after.encode(),
        }
    }

    fn into_records(self) -> Vec<(u64, SyncRecord)> {
        self.records.into_values().collect()
    }
}

fn next_historical_backfill_page_cursor(
    collected: &HistoricalBackfillCollector,
    page_len: usize,
    after: Option<&HistoricalBackfillSortKey>,
) -> anyhow::Result<Option<String>> {
    (collected.has_more || page_len < collected.records.len())
        .then(|| collected.encode_cursor_after(after.expect("nonempty historical backfill page")))
        .transpose()
}

fn serialized_backfill_record_bytes(record: &SyncRecord) -> anyhow::Result<usize> {
    Ok(serde_json::to_vec(&RepositoryInitialBackfillRecord {
        observed_at_unix_seconds: 0,
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
        tombstone: record.is_tombstone(),
    })?
    .len())
}

impl RepositoryInitialBackfillRecord {
    fn into_sync_record(self) -> anyhow::Result<(u64, SyncRecord)> {
        Ok((
            self.observed_at_unix_seconds,
            SyncRecord::new(
                self.subject_node_id,
                self.observer_node_id,
                self.schema_id,
                self.schema_version,
                URL_SAFE_NO_PAD.decode(self.record_key_base64)?,
                URL_SAFE_NO_PAD.decode(self.payload_base64)?,
                self.tombstone,
            ),
        ))
    }

    pub(crate) fn into_tiered_backfill_record(
        self,
    ) -> anyhow::Result<
        Option<crate::state::history_repository::replica::RepositoryTieredBackfillRecord>,
    > {
        let Self {
            observed_at_unix_seconds,
            source_node_id,
            source_epoch,
            stream,
            sequence,
            subject_node_id,
            observer_node_id,
            schema_id,
            schema_version,
            record_key_base64,
            payload_base64,
            tombstone,
        } = self;
        match (source_node_id, source_epoch, stream, sequence) {
            (None, None, None, None) => Ok(None),
            (Some(source_node_id), Some(source_epoch), Some(stream), Some(sequence)) => Ok(Some(
                crate::state::history_repository::replica::RepositoryTieredBackfillRecord {
                    observed_at_unix_seconds,
                    source_node_id,
                    source_epoch,
                    stream,
                    sequence,
                    subject_node_id,
                    observer_node_id,
                    schema_id,
                    schema_version,
                    record_key: URL_SAFE_NO_PAD.decode(record_key_base64)?,
                    payload: URL_SAFE_NO_PAD.decode(payload_base64)?,
                    tombstone,
                },
            )),
            _ => anyhow::bail!("initial history backfill record has a partial source cursor"),
        }
    }
}
pub(crate) async fn backfill_initial_repository_from_local_history(
    state: &AppState,
    _now: u64,
) -> anyhow::Result<InitialBackfillProgress> {
    let local_backfill_completed = {
        let mut replica = state.repository_replica.lock().await;
        replica.prepare_local_history_backfill(_now)?;
        replica.local_history_backfill_completed()
            && replica
                .local_history_backfill_inflight_checkpoint()
                .is_none()
    };
    let node_id = RepositoryNodeId::try_from(state.cluster.node_id.clone())?;
    let identity = derived_repository_identity(state, node_id)
        .map_err(|_| anyhow::anyhow!("derive local history backfill identity"))?;
    let signing_key = derived_repository_signing_key(state, identity.node_id().as_str())
        .map_err(|_| anyhow::anyhow!("derive local history backfill signing key"))?;
    let mut ready_repository_ids = {
        let store = state.store.lock().await;
        store
            .state()
            .repository_membership
            .as_ref()
            .map(|membership| {
                membership
                    .ready_members()
                    .map(|member| member.node_id().as_str().to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let has_ready_repository = !ready_repository_ids.is_empty();
    if !ready_repository_ids
        .iter()
        .any(|id| id == &state.cluster.node_id)
    {
        ready_repository_ids.push(state.cluster.node_id.clone());
    }
    let mut progress = InitialBackfillProgress::Complete;
    if !has_ready_repository {
        for peer in initial_history_source_peers(state).await {
            progress = progress
                .combine(pull_peer_initial_history(state, &peer, &ready_repository_ids).await?);
        }
    }
    // A first repository must learn every registered source's deletion baseline before replay.
    // Durable peer cursors bound each tick; an unavailable source never becomes an implicit ACK.
    if !has_ready_repository && progress != InitialBackfillProgress::Complete {
        return Ok(progress);
    }
    if !local_backfill_completed {
        let inflight = state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_inflight_checkpoint();
        if let Some((page_cursor, completed)) = inflight {
            let page_progress = local_page::resume_local_history_backfill_page(
                state,
                &identity,
                _now,
                &ready_repository_ids,
                page_cursor,
                completed,
            )
            .await?;
            progress = progress.combine(page_progress);
        } else {
            let collection_state = state
                .repository_replica
                .lock()
                .await
                .local_history_backfill_collection_state();
            let collected = historical_source_backfill_records(
                state,
                collection_state.0.as_deref(),
                MAX_INITIAL_BACKFILL_PAGE_RECORDS,
            )
            .await?;
            let page_cursor = collected.next_cursor()?;
            let completed = page_cursor.is_none();
            let batches = historical_record_batches(collected.into_records())?;
            let mut replica = state.repository_replica.lock().await;
            replica.validate_local_history_backfill_collection_state(&collection_state)?;
            if batches.iter().any(|(_, records)| !records.is_empty()) {
                replica.apply_local_history_backfill_page(
                    &state.cluster.cluster_id,
                    identity.clone(),
                    &signing_key,
                    page_cursor.clone(),
                    completed,
                    queued_history_backfill_batches(batches),
                    _now,
                    &ready_repository_ids,
                    &state.cluster.node_id,
                )?;
            } else {
                replica.checkpoint_local_history_backfill(page_cursor.clone(), completed)?;
            }
            drop(replica);
            progress = progress.combine(if completed {
                InitialBackfillProgress::Complete
            } else {
                InitialBackfillProgress::InProgress
            });
        }
    }
    Ok(progress)
}

async fn initial_history_source_peers(state: &AppState) -> Vec<MeshPeerTarget> {
    let store = state.store.lock().await;
    let endpoints = store.list_endpoints();
    store
        .list_nodes()
        .into_iter()
        // A configured peer serves its node-local history from this endpoint. Including it here
        // makes concurrently syncing repositories ingest the same complete cluster baseline;
        // it does not recursively start repository-to-repository repair.
        .filter(|node| is_initial_history_source_peer(&node.node_id, &state.cluster.node_id))
        .map(|node| peer_target_from_node(&node, &endpoints))
        .collect()
}

fn is_initial_history_source_peer(peer_node_id: &str, local_node_id: &str) -> bool {
    peer_node_id != local_node_id
}
pub(super) async fn pull_peer_initial_history(
    state: &AppState,
    peer: &MeshPeerTarget,
    ready_repository_ids: &[String],
) -> anyhow::Result<InitialBackfillProgress> {
    // Imported pre-repository history belongs to the repository that durably observed it. The
    // original node remains the subject and is encoded in the stable key. This lets that
    // repository's signed deletion marker remove peer history without impersonating a peer.
    let node_id = RepositoryNodeId::try_from(state.cluster.node_id.clone())?;
    let identity = derived_repository_identity(state, node_id)
        .map_err(|_| anyhow::anyhow!("derive local peer-import identity"))?;
    let signing_key = derived_repository_signing_key(state, identity.node_id().as_str())
        .map_err(|_| anyhow::anyhow!("derive local peer-import signing key"))?;
    let epoch = state
        .repository_replica
        .lock()
        .await
        .initial_peer_backfill_epoch(
            &state.cluster.cluster_id,
            identity.node_id().as_str(),
            &peer.node_id,
        )?;
    let checkpoint = state
        .repository_replica
        .lock()
        .await
        .initial_peer_backfill_checkpoint(&peer.node_id)
        .unwrap_or_default();
    let cursor = checkpoint.page_cursor;
    let mut stream_state = checkpoint.stream_state;
    let mut saw_history = checkpoint.saw_history;
    if checkpoint.completed {
        return Ok(InitialBackfillProgress::Complete);
    }
    let page_body = match repository_direct_request_body(
        state,
        peer,
        Method::GET,
        &cursor.as_deref().map_or_else(
            || {
                format!(
                    "/api/admin/_internal/history-repository/initial-backfill?page_size={}",
                    MAX_INITIAL_BACKFILL_PAGE_RECORDS
                )
            },
            |cursor| {
                format!(
                    "{}?page_cursor={cursor}&page_size={MAX_INITIAL_BACKFILL_PAGE_RECORDS}",
                    "/api/admin/_internal/history-repository/initial-backfill"
                )
            },
        ),
        Vec::new(),
        MAX_INITIAL_BACKFILL_WIRE_BYTES,
    )
    .await
    {
        Ok(page) => page,
        Err(error) => {
            if should_restart_peer_backfill(cursor.as_deref(), &error) {
                state
                    .repository_replica
                    .lock()
                    .await
                    .restart_initial_peer_backfill(&peer.node_id)?;
            }
            tracing::debug!(
                peer = %peer.node_id,
                error = %error,
                "peer history backfill is incomplete"
            );
            return Ok(InitialBackfillProgress::Unavailable);
        }
    };
    let page = match deserialize_initial_backfill_page(&page_body) {
        Ok(page) => page,
        Err(error) => {
            tracing::debug!(
                peer = %peer.node_id,
                error = %error,
                "peer history backfill page failed pre-decode validation"
            );
            return Ok(InitialBackfillProgress::Unavailable);
        }
    };
    if let Err(error) =
        ordering::validate_initial_source_page(&page, cursor.as_deref(), &state.cluster.cluster_id)
    {
        tracing::debug!(
            peer = %peer.node_id,
            error = %error,
            "peer history backfill page failed semantic validation"
        );
        return Ok(InitialBackfillProgress::Unavailable);
    }
    if !page.records.is_empty() {
        saw_history = true;
        receive_peer_backfill_page(
            state,
            PeerBackfillImport {
                identity: &identity,
                signing_key: &signing_key,
                peer_node_id: &peer.node_id,
                epoch,
                ready_repository_ids,
            },
            page.records,
            &mut stream_state,
        )
        .await?;
    }
    let Some(next_page_cursor) = page.next_page_cursor else {
        state
            .repository_replica
            .lock()
            .await
            .update_initial_peer_backfill_checkpoint(
                &peer.node_id,
                None,
                stream_state,
                saw_history,
                true,
            )?;
        return Ok(InitialBackfillProgress::Complete);
    };
    state
        .repository_replica
        .lock()
        .await
        .update_initial_peer_backfill_checkpoint(
            &peer.node_id,
            Some(next_page_cursor),
            stream_state,
            saw_history,
            false,
        )?;
    Ok(InitialBackfillProgress::InProgress)
}
pub(crate) fn should_restart_peer_backfill(
    cursor: Option<&str>,
    error: &RepositoryDirectError,
) -> bool {
    cursor.is_some()
        && matches!(error, RepositoryDirectError::Application(_))
        && !ordering::is_transient_source_rejection(error)
}
pub(super) async fn receive_peer_backfill_page(
    state: &AppState,
    import: PeerBackfillImport<'_>,
    records: Vec<RepositoryInitialBackfillRecord>,
    stream_state: &mut BTreeMap<String, (u64, Option<[u8; 32]>)>,
) -> anyhow::Result<()> {
    let mut grouped = BTreeMap::<(u64, String), Vec<SyncRecord>>::new();
    let mut tiered_records = Vec::new();
    for record in records {
        if let Some(record) = record.clone().into_tiered_backfill_record()? {
            tiered_records.push(record);
            continue;
        }
        let (observed_at, record) = record.into_sync_record()?;
        let stream = peer_backfill_stream_for_record(&record, import.peer_node_id)?;
        grouped
            .entry((observed_at, stream))
            .or_default()
            .push(record);
    }
    if !tiered_records.is_empty() {
        state
            .repository_replica
            .lock()
            .await
            .import_tiered_backfill_records(
                tiered_records,
                u64::try_from(chrono::Utc::now().timestamp()).unwrap_or_default(),
                import.ready_repository_ids,
                &state.cluster.node_id,
            )?;
    }
    for ((observed_at, stream), records) in grouped {
        for records in records.chunks(32) {
            let (next_sequence, previous_hash) =
                stream_state.entry(stream.clone()).or_insert((0, None));
            let signed = CanonicalSegment::new(
                &state.cluster.cluster_id,
                Cursor::new(
                    import.identity.node_id().as_str(),
                    import.epoch,
                    &stream,
                    *next_sequence,
                )?,
                records.to_vec(),
                *previous_hash,
                observed_at,
                observed_at,
            )?
            .sign(import.signing_key)?;
            let wire = signed.wire_bytes()?;
            *next_sequence = next_sequence.saturating_add(u64::try_from(records.len())?);
            *previous_hash = Some(signed.segment_hash()?);
            state
                .repository_replica
                .lock()
                .await
                .receive_repair_wire_from_repository_with_gaps(
                    &state.cluster.cluster_id,
                    import.identity,
                    &wire,
                    &[],
                    observed_at,
                    import.ready_repository_ids,
                    &state.cluster.node_id,
                )?;
        }
    }
    Ok(())
}
pub(crate) fn source_stream_for_schema(schema_id: &str) -> Option<&'static str> {
    Some(match schema_id {
        "runtime.v1" => "runtime",
        "path_health.v1" => "path_health",
        "traffic.v1" => "traffic",
        "connections.v1" => "connections",
        "ip_usage.v1" => "ip_usage",
        crate::resource_monitoring::RESOURCE_HISTORY_SCHEMA => {
            crate::resource_monitoring::RESOURCE_HISTORY_STREAM
        }
        _ => return None,
    })
}
pub(crate) fn peer_backfill_stream_for_schema(
    schema_id: &str,
    peer_node_id: &str,
) -> anyhow::Result<String> {
    let stream = source_stream_for_schema(schema_id)
        .ok_or_else(|| anyhow::anyhow!("peer history has an unsupported source schema"))?;
    Ok(format!("{stream}-backfill-{peer_node_id}"))
}
pub(crate) fn peer_backfill_stream_for_record(
    record: &SyncRecord,
    peer_node_id: &str,
) -> anyhow::Result<String> {
    if record.is_tombstone() {
        Ok(format!("tombstone-backfill-{peer_node_id}"))
    } else {
        peer_backfill_stream_for_schema(record.schema().0, peer_node_id)
    }
}
pub(crate) async fn initial_backfill_page_stable(
    state: &AppState,
    page_cursor: Option<&str>,
    page_size: Option<usize>,
    stable_deletions: bool,
) -> anyhow::Result<RepositoryInitialBackfillPage> {
    let page_size = page_size.unwrap_or(MAX_INITIAL_BACKFILL_PAGE_RECORDS);
    if page_size == 0 || page_size > MAX_INITIAL_BACKFILL_PAGE_RECORDS {
        anyhow::bail!("initial history backfill page size exceeds limit");
    }
    let local_is_ready_repository = {
        let store = state.store.lock().await;
        store
            .state()
            .repository_membership
            .as_ref()
            .and_then(|membership| {
                RepositoryNodeId::try_from(state.cluster.node_id.clone())
                    .ok()
                    .and_then(|node_id| membership.repository(&node_id))
            })
            .is_some_and(|member| member.lifecycle() == &RepositoryLifecycle::Ready)
    };
    if stable_deletions {
        anyhow::ensure!(
            !local_is_ready_repository,
            "stable source deletion export requires a non-Ready source"
        );
        return source_deletions::page(state, page_cursor, page_size).await;
    }
    if local_is_ready_repository {
        let now_unix_seconds = u64::try_from(chrono::Utc::now().timestamp()).unwrap_or_default();
        let repair_cache_cutoff_unix_seconds = now_unix_seconds.saturating_sub(
            crate::state::history_repository::replica::RepositoryRetentionPolicy::default()
                .minute_retention_seconds(),
        );
        let page = state.repository_replica.lock().await.tiered_backfill_page(
            page_cursor,
            page_size,
            repair_cache_cutoff_unix_seconds,
            now_unix_seconds,
        )?;
        return Ok(RepositoryInitialBackfillPage {
            records: page
                .records
                .into_iter()
                .map(|record| RepositoryInitialBackfillRecord {
                    observed_at_unix_seconds: record.observed_at_unix_seconds,
                    source_node_id: Some(record.source_node_id),
                    source_epoch: Some(record.source_epoch),
                    stream: Some(record.stream),
                    sequence: Some(record.sequence),
                    subject_node_id: record.subject_node_id,
                    observer_node_id: record.observer_node_id,
                    schema_id: record.schema_id,
                    schema_version: record.schema_version,
                    record_key_base64: URL_SAFE_NO_PAD.encode(record.record_key),
                    payload_base64: URL_SAFE_NO_PAD.encode(record.payload),
                    tombstone: record.tombstone,
                })
                .collect(),
            next_page_cursor: page.next_cursor,
            deletion_snapshot: None,
        });
    }
    let collected = historical_source_backfill_records(state, page_cursor, page_size).await?;
    let mut page = Vec::new();
    let mut next_cursor = None;
    for (sort_key, (observed_at_unix_seconds, record)) in &collected.records {
        let candidate = RepositoryInitialBackfillRecord {
            observed_at_unix_seconds: *observed_at_unix_seconds,
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
            tombstone: record.is_tombstone(),
        };
        let candidate_bytes = serde_json::to_vec(&candidate)?.len();
        let page_bytes = serde_json::to_vec(&page)?.len();
        if !page.is_empty()
            && page_bytes.saturating_add(candidate_bytes) > MAX_INITIAL_BACKFILL_PAGE_BYTES
        {
            break;
        }
        if candidate_bytes > MAX_INITIAL_BACKFILL_PAGE_BYTES {
            anyhow::bail!("initial history backfill record exceeds page budget");
        }
        page.push(candidate);
        next_cursor = Some(sort_key.clone());
    }
    let next_page_cursor =
        next_historical_backfill_page_cursor(&collected, page.len(), next_cursor.as_ref())?;
    Ok(RepositoryInitialBackfillPage {
        records: page,
        next_page_cursor,
        deletion_snapshot: collected.deletion_snapshot,
    })
}
fn push_backfill_record(
    records: &mut HistoricalBackfillCollector,
    schema_id: &str,
    node_id: &str,
    observed_at: &str,
    record_key: String,
    payload: serde_json::Value,
) -> anyhow::Result<()> {
    let observed_at = chrono::DateTime::parse_from_rfc3339(observed_at)
        .map_err(|error| {
            anyhow::anyhow!("parse persisted history timestamp {observed_at}: {error}")
        })?
        .timestamp()
        .try_into()
        .map_err(|_| anyhow::anyhow!("persisted history timestamp is before Unix epoch"))?;
    records.push((
        observed_at,
        source_record_with_key(
            schema_id,
            node_id,
            observed_at,
            record_key.into_bytes(),
            payload,
            false,
        )?,
    ))?;
    Ok(())
}
pub(super) fn historical_record_batches(
    records: Vec<(u64, SyncRecord)>,
) -> anyhow::Result<Vec<(u64, Vec<SyncRecord>)>> {
    const MAX_RECORDS_PER_BACKFILL_SEGMENT: usize = 32;
    let mut batches = Vec::new();
    let mut current_timestamp = None;
    let mut current_stream = None;
    let mut current_records = Vec::new();
    for (observed_at, record) in records {
        let stream = if record.is_tombstone() {
            "tombstone"
        } else {
            source_stream_for_schema(record.schema().0)
                .ok_or_else(|| anyhow::anyhow!("unknown backfill schema"))?
        };
        let timestamp_changed = current_timestamp != Some(observed_at);
        let stream_changed = current_stream != Some(stream);
        let record_limit = current_records.len() == MAX_RECORDS_PER_BACKFILL_SEGMENT;
        let exceeds_byte_limit = if current_records.is_empty()
            || (!timestamp_changed && !stream_changed && !record_limit)
        {
            let mut candidate = current_records.clone();
            candidate.push(record.clone());
            backfill_segment_canonical_size(observed_at, stream, &candidate)?
                > MAX_INITIAL_BACKFILL_PAGE_BYTES
        } else {
            false
        };
        if timestamp_changed || stream_changed || record_limit || exceeds_byte_limit {
            if current_records.is_empty() {
                if exceeds_byte_limit {
                    anyhow::bail!("initial history backfill segment exceeds byte budget");
                }
            } else {
                batches.push((
                    current_timestamp.expect("backfill batch timestamp"),
                    std::mem::take(&mut current_records),
                ));
            }
            current_timestamp = Some(observed_at);
            current_stream = Some(stream);
        }
        current_records.push(record);
    }
    if let Some(timestamp) = current_timestamp {
        batches.push((timestamp, current_records));
    }
    Ok(batches)
}

pub(super) fn queued_history_backfill_batches(
    batches: Vec<(u64, Vec<SyncRecord>)>,
) -> Vec<(Vec<SyncRecord>, u64)> {
    batches
        .into_iter()
        .map(|(observed_at_unix_seconds, records)| (records, observed_at_unix_seconds))
        .collect()
}

fn backfill_segment_canonical_size(
    timestamp: u64,
    stream: &str,
    records: &[SyncRecord],
) -> anyhow::Result<usize> {
    let result = CanonicalSegment::new(
        "backfill",
        Cursor::new("backfill", 0, stream, 0)?,
        records.to_vec(),
        None,
        timestamp,
        timestamp,
    )
    .and_then(|segment| segment.canonical_bytes());
    match result {
        Ok(bytes) => Ok(bytes.len()),
        Err(ProtocolError::SegmentCanonicalLimit { actual }) => Ok(actual),
        Err(error) => Err(error.into()),
    }
}
