use super::*;
use crate::history_sync::ProtocolError;
use crate::http::history_repository::{
    derived_repository_identity, derived_repository_signing_key,
};
use crate::state::history_repository::{
    MAX_INITIAL_BACKFILL_PAGE_BYTES, MAX_INITIAL_BACKFILL_PAGE_RECORDS,
};
mod ready_peer;

pub(crate) use ready_peer::catch_up_against_ready_repositories;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InitialBackfillProgress {
    InProgress,
    Complete,
    Unavailable,
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
}
pub(crate) struct HistoricalBackfillCollector {
    pub(crate) after: Option<HistoricalBackfillSortKey>,
    pub(crate) limit: usize,
    pub(crate) records: BTreeMap<HistoricalBackfillSortKey, (u64, SyncRecord)>,
    pub(crate) has_more: bool,
}
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub(crate) struct HistoricalBackfillSortKey {
    observed_at_unix_seconds: u64,
    schema_id: String,
    #[serde(with = "backfill_cursor_key")]
    record_key: Vec<u8>,
}
impl HistoricalBackfillCollector {
    pub(crate) fn new(after: Option<HistoricalBackfillSortKey>, limit: usize) -> Self {
        Self {
            after,
            limit,
            records: BTreeMap::new(),
            has_more: false,
        }
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
        if self.records.len() == self.limit {
            let bytes = self
                .records
                .values()
                .map(|(_, record)| serialized_backfill_record_bytes(record))
                .sum::<anyhow::Result<usize>>()?;
            if bytes > MAX_INITIAL_BACKFILL_PAGE_BYTES {
                while self.records.len() > 1
                    && self
                        .records
                        .values()
                        .map(|(_, record)| serialized_backfill_record_bytes(record))
                        .sum::<anyhow::Result<usize>>()?
                        > MAX_INITIAL_BACKFILL_PAGE_BYTES
                {
                    self.records.pop_last();
                    self.has_more = true;
                }
            }
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
    pub(crate) fn next_cursor(&self) -> anyhow::Result<Option<String>> {
        self.has_more
            .then(|| {
                self.records
                    .last_key_value()
                    .expect("backfill cursor requires a record")
                    .0
                    .encode()
            })
            .transpose()
    }
    fn into_records(self) -> Vec<(u64, SyncRecord)> {
        self.records.into_values().collect()
    }
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
mod backfill_cursor_key {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&URL_SAFE_NO_PAD.encode(value))
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(serde::de::Error::custom)
    }
}

impl HistoricalBackfillSortKey {
    pub(crate) fn encode(&self) -> anyhow::Result<String> {
        Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(self)?))
    }

    pub(crate) fn decode(encoded: &str) -> anyhow::Result<Self> {
        if encoded.len() > 1_024 {
            anyhow::bail!("initial history backfill cursor exceeds limit");
        }
        Ok(serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded)?)?)
    }
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

    fn into_tiered_backfill_record(
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
    let local_backfill_completed = state
        .repository_replica
        .lock()
        .await
        .local_history_backfill_completed();
    let node_id = RepositoryNodeId::try_from(state.cluster.node_id.clone())?;
    let identity = derived_repository_identity(state, node_id)
        .map_err(|_| anyhow::anyhow!("derive local history backfill identity"))?;
    let signing_key = derived_repository_signing_key(state, identity.node_id().as_str())
        .map_err(|_| anyhow::anyhow!("derive local history backfill signing key"))?;
    let ready_repository_ids = vec![state.cluster.node_id.clone()];
    if !local_backfill_completed {
        let inflight = state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_inflight_checkpoint();
        let (segments, page_cursor, completed) = if let Some((page_cursor, completed)) = inflight {
            (
                state
                    .repository_replica
                    .lock()
                    .await
                    .local_history_backfill_inflight_segments()?,
                page_cursor,
                completed,
            )
        } else {
            let local_cursor = state
                .repository_replica
                .lock()
                .await
                .local_history_backfill_cursor()
                .map(ToOwned::to_owned);
            let collected = historical_source_backfill_records(
                state,
                local_cursor.as_deref(),
                MAX_INITIAL_BACKFILL_PAGE_RECORDS,
            )
            .await?;
            let page_cursor = collected.next_cursor()?;
            let completed = page_cursor.is_none();
            let batches = historical_record_batches(collected.into_records())?;
            if batches.iter().any(|(_, records)| !records.is_empty()) {
                let segments = state
                    .repository_replica
                    .lock()
                    .await
                    .queue_local_history_backfill_batches(
                        &state.cluster.cluster_id,
                        identity.clone(),
                        &signing_key,
                        page_cursor.clone(),
                        completed,
                        queued_history_backfill_batches(batches),
                    )?;
                (segments, page_cursor, completed)
            } else {
                state
                    .repository_replica
                    .lock()
                    .await
                    .checkpoint_local_history_backfill(page_cursor.clone(), completed)?;
                (Vec::new(), page_cursor, completed)
            }
        };
        for segment in &segments {
            receive_local_source_segment(state, segment, &[], &ready_repository_ids, _now).await?;
        }
        if !segments.is_empty() {
            state
                .repository_replica
                .lock()
                .await
                .acknowledge_local_source_segments_and_checkpoint_backfill(
                    &segments,
                    page_cursor,
                    completed,
                )?;
        }
        if !completed {
            return Ok(InitialBackfillProgress::InProgress);
        }
        // Peer availability must not replay local pages on every retry. The source outbox has
        // already durably acknowledged every local segment at this point.
    }
    for peer in all_cluster_peers(state).await {
        match pull_peer_initial_history(state, &peer, &ready_repository_ids).await? {
            InitialBackfillProgress::InProgress => {
                return Ok(InitialBackfillProgress::InProgress);
            }
            InitialBackfillProgress::Unavailable => {
                return Ok(InitialBackfillProgress::Unavailable);
            }
            InitialBackfillProgress::Complete => {}
        }
    }
    Ok(InitialBackfillProgress::Complete)
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
    let page: RepositoryInitialBackfillPage = match repository_direct_request(
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
    if cursor.as_deref() == Some(next_page_cursor.as_str()) {
        anyhow::bail!("peer history backfill page cursor did not advance");
    }
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
    cursor.is_some() && matches!(error, RepositoryDirectError::Application(_))
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
                .receive_wire_from_repository(
                    &state.cluster.cluster_id,
                    import.identity,
                    &wire,
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
        Ok("tombstone".to_owned())
    } else {
        peer_backfill_stream_for_schema(record.schema().0, peer_node_id)
    }
}
pub(crate) async fn initial_backfill_page(
    state: &AppState,
    page_cursor: Option<&str>,
    page_size: Option<usize>,
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
    let next_page_cursor = (collected.has_more || page.len() < collected.records.len())
        .then(|| {
            next_cursor
                .as_ref()
                .expect("nonempty backfill page")
                .encode()
        })
        .transpose()?;
    Ok(RepositoryInitialBackfillPage {
        records: page,
        next_page_cursor,
    })
}
async fn historical_source_backfill_records(
    state: &AppState,
    page_cursor: Option<&str>,
    limit: usize,
) -> anyhow::Result<HistoricalBackfillCollector> {
    let after = page_cursor
        .map(HistoricalBackfillSortKey::decode)
        .transpose()?;
    let mut records = HistoricalBackfillCollector::new(after, limit);
    if let Some(history) = state.node_history.snapshot(&state.cluster.node_id).await {
        let history_node_id = history.node_id.clone();
        for traffic in history.daily_traffic {
            push_backfill_record(
                &mut records,
                "traffic.v1",
                &state.cluster.node_id,
                &format!("{}T00:00:00Z", traffic.date),
                format!(
                    "node-history:node:{history_node_id}:daily-traffic:{}",
                    traffic.date
                ),
                serde_json::to_value(traffic)?,
            )?;
        }
        for status in history.daily_component_status {
            let date = status.date.clone();
            push_backfill_record(
                &mut records,
                "runtime.v1",
                &state.cluster.node_id,
                &format!("{date}T00:00:00Z"),
                format!("node-history:node:{history_node_id}:daily-status:{date}"),
                serde_json::to_value(status)?,
            )?;
        }
        for event in history.component_status_events {
            let occurred_at = event.occurred_at.clone();
            let key = event.event_id.clone();
            push_backfill_record(
                &mut records,
                "path_health.v1",
                &state.cluster.node_id,
                &occurred_at,
                format!("node-history:node:{history_node_id}:event:{key}"),
                serde_json::to_value(event)?,
            )?;
        }
        if let Some(traffic) = history.traffic {
            for bucket in traffic.five_minute {
                let at = bucket.end_at.clone();
                let key = bucket.start_at.clone();
                push_backfill_record(
                    &mut records,
                    "traffic.v1",
                    &state.cluster.node_id,
                    &at,
                    format!("node-history:node:{history_node_id}:five-minute:{key}"),
                    serde_json::to_value(bucket)?,
                )?;
            }
            for bucket in traffic.daily {
                let date = bucket.date.clone();
                push_backfill_record(
                    &mut records,
                    "traffic.v1",
                    &state.cluster.node_id,
                    &format!("{date}T00:00:00Z"),
                    format!("node-history:node:{history_node_id}:daily-rollup:{date}"),
                    serde_json::to_value(bucket)?,
                )?;
            }
        }
    }
    let mesh = state.mesh_telemetry.snapshot().await;
    for peer in mesh.peers {
        let peer_id = peer.peer_id.clone();
        for bucket in peer.buckets {
            let minute = bucket.minute.clone();
            push_backfill_record(
                &mut records,
                "path_health.v1",
                &state.cluster.node_id,
                &minute,
                format!(
                    "node-history:node:{}:mesh:{peer_id}:{minute}",
                    state.cluster.node_id
                ),
                serde_json::to_value(bucket)?,
            )?;
        }
    }
    let (inbound_samples, connection_samples) = {
        let store = state.store.lock().await;
        (
            store
                .inbound_ip_usage()
                .repository_samples_for_node(&state.cluster.node_id),
            store
                .tcp_connection_usage()
                .repository_samples_for_node(&state.cluster.node_id),
        )
    };
    for sample in inbound_samples {
        let minute = sample.minute.clone();
        push_backfill_record(
            &mut records,
            "ip_usage.v1",
            &state.cluster.node_id,
            &minute,
            format!(
                "node-history:node:{}:inbound-ip:{minute}",
                state.cluster.node_id
            ),
            serde_json::to_value(sample)?,
        )?;
    }
    for endpoint in connection_samples {
        let endpoint_id = endpoint.endpoint_id.clone();
        for sample in endpoint.series {
            let minute = sample.minute.clone();
            push_backfill_record(
                &mut records,
                "connections.v1",
                &state.cluster.node_id,
                &minute,
                format!(
                    "node-history:node:{}:tcp:{endpoint_id}:{minute}",
                    state.cluster.node_id
                ),
                serde_json::to_value(sample)?,
            )?;
        }
    }
    // A node can be asked for its historical window while a deletion is still waiting for its
    // first repository acknowledgement. Include the independent tombstone stream in that same
    // bounded export so a joining repository never treats the old window as resurrectable.
    let deletion_markers = state
        .node_history
        .repository_deletion_markers(&state.cluster.node_id)
        .await;
    let now_unix_seconds = u64::try_from(chrono::Utc::now().timestamp()).unwrap_or_default();
    for tombstone in source_records_with_deletions(
        &state.cluster.node_id,
        now_unix_seconds,
        deletion_markers
            .into_iter()
            .map(|marker| {
                let target_node_id = marker.target_node_id().map(str::to_owned);
                (marker.schema_id, marker.record_key, target_node_id)
            })
            .collect(),
        Vec::new(),
    )? {
        // Backfill has no source cursor before the repository protocol begins. Reserve the
        // first collector phase for deletion intent so no historical page can arrive before its
        // matching tombstone stream has been accepted.
        records.push_with_times(0, now_unix_seconds, tombstone)?;
    }
    Ok(records)
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
