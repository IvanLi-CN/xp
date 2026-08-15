use super::*;
use crate::http::history_repository::{
    derived_repository_identity, derived_repository_signing_key,
};

const CATCH_UP_PEER_VERIFICATION_ATTEMPTS: usize = 2;

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

    pub(crate) fn push(&mut self, record: (u64, SyncRecord)) {
        let key = HistoricalBackfillSortKey {
            observed_at_unix_seconds: record.0,
            schema_id: record.1.schema().0.to_owned(),
            record_key: record.1.record_key().to_vec(),
        };
        if self.after.as_ref().is_some_and(|after| key <= *after) {
            return;
        }
        self.records.insert(key, record);
        if self.records.len() > self.limit {
            self.records.pop_last();
            self.has_more = true;
        }
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
) -> anyhow::Result<bool> {
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
    let mut saw_history = false;
    let ready_repository_ids = vec![state.cluster.node_id.clone()];
    if !local_backfill_completed {
        let mut local_cursor = state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_cursor()
            .map(ToOwned::to_owned);
        loop {
            let collected =
                historical_source_backfill_records(state, local_cursor.as_deref(), 1).await?;
            saw_history |= !collected.records.is_empty();
            let next_cursor = collected.next_cursor()?;
            let completed = next_cursor.is_none();
            let batches = historical_record_batches(collected.into_records());
            let last_batch = batches.len().saturating_sub(1);
            let mut checkpointed = false;
            for (batch_index, (observed_at, records)) in batches.into_iter().enumerate() {
                let segments = state
                    .repository_replica
                    .lock()
                    .await
                    .queue_local_history_backfill_segments(
                        &state.cluster.cluster_id,
                        identity.clone(),
                        &signing_key,
                        records,
                        observed_at,
                    )?;
                let last_segment = segments.len().saturating_sub(1);
                for (segment_index, segment) in segments.iter().enumerate() {
                    receive_local_source_segment(
                        state,
                        segment,
                        &[],
                        &ready_repository_ids,
                        observed_at,
                    )
                    .await?;
                    let mut runtime = state.repository_replica.lock().await;
                    if batch_index == last_batch && segment_index == last_segment {
                        runtime.acknowledge_local_source_segment_and_checkpoint_backfill(
                            &segment.wire,
                            next_cursor.clone(),
                            completed,
                        )?;
                        checkpointed = true;
                    } else {
                        runtime.acknowledge_local_source_segment(&segment.wire)?;
                    }
                }
            }
            if !checkpointed {
                state
                    .repository_replica
                    .lock()
                    .await
                    .checkpoint_local_history_backfill(next_cursor.clone(), completed)?;
            }
            let Some(next_cursor) = next_cursor else {
                break;
            };
            local_cursor = Some(next_cursor);
        }
        // Peer availability must not replay local pages on every retry. The source outbox has
        // already durably acknowledged every local segment at this point.
    }
    let mut peer_backfill_statuses = Vec::new();
    for peer in all_cluster_peers(state).await {
        let peer_history = pull_peer_initial_history(state, &peer, &ready_repository_ids).await?;
        saw_history |= peer_history.unwrap_or_default();
        peer_backfill_statuses.push(peer_history);
    }
    if !initial_backfill_is_complete(saw_history, &peer_backfill_statuses) {
        // A first repository with no migrated history is deliberately not "caught up". Its
        // lifecycle remains syncing instead of publishing a false complete/local-only window.
        return Ok(false);
    }
    Ok(true)
}

pub(crate) fn initial_backfill_is_complete(
    _saw_history: bool,
    peer_backfill_statuses: &[Option<bool>],
) -> bool {
    // An all-empty cluster is a valid zero-coverage baseline. It is only incomplete when a
    // peer has not responded, because that leaves historic coverage unknown.
    peer_backfill_statuses.iter().all(Option::is_some)
}

pub(super) async fn pull_peer_initial_history(
    state: &AppState,
    peer: &MeshPeerTarget,
    ready_repository_ids: &[String],
) -> anyhow::Result<Option<bool>> {
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
    let mut cursor = checkpoint.page_cursor;
    let mut stream_state = checkpoint.stream_state;
    let mut saw_history = checkpoint.saw_history;
    if checkpoint.completed {
        return Ok(Some(saw_history));
    }
    loop {
        let page: RepositoryInitialBackfillPage = match repository_direct_request(
            state,
            peer,
            Method::GET,
            &cursor.as_deref().map_or_else(
                || {
                    "/api/admin/_internal/history-repository/initial-backfill?page_size=1"
                        .to_owned()
                },
                |cursor| {
                    format!(
                        "{}?page_cursor={cursor}&page_size=1",
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
                return Ok(None);
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
            return Ok(Some(saw_history));
        };
        if cursor.as_deref() == Some(next_page_cursor.as_str()) {
            anyhow::bail!("peer history backfill page cursor did not advance");
        }
        cursor = Some(next_page_cursor);
        state
            .repository_replica
            .lock()
            .await
            .update_initial_peer_backfill_checkpoint(
                &peer.node_id,
                cursor.clone(),
                stream_state.clone(),
                saw_history,
                false,
            )?;
    }
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
        records.push((0, tombstone));
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
    ));
    Ok(())
}

pub(super) fn historical_record_batches(
    records: Vec<(u64, SyncRecord)>,
) -> Vec<(u64, Vec<SyncRecord>)> {
    const MAX_RECORDS_PER_BACKFILL_SEGMENT: usize = 32;
    let mut batches = Vec::new();
    let mut current_timestamp = None;
    let mut current_records = Vec::new();
    for (observed_at, record) in records {
        if current_timestamp != Some(observed_at)
            || current_records.len() == MAX_RECORDS_PER_BACKFILL_SEGMENT
        {
            if let Some(timestamp) = current_timestamp {
                batches.push((timestamp, std::mem::take(&mut current_records)));
            }
            current_timestamp = Some(observed_at);
        }
        current_records.push(record);
    }
    if let Some(timestamp) = current_timestamp {
        batches.push((timestamp, current_records));
    }
    batches
}

pub(crate) async fn catch_up_against_ready_repositories(
    state: &AppState,
    now: u64,
) -> anyhow::Result<bool> {
    let (ready_repository_ids, peers) = ready_repository_peers(state).await?;
    if peers.len() != ready_repository_ids.len() {
        return Ok(false);
    }
    let mut receiving_repository_ids = ready_repository_ids.clone();
    receiving_repository_ids.push(state.cluster.node_id.clone());
    receiving_repository_ids.sort_unstable();
    receiving_repository_ids.dedup();
    {
        let mut runtime = state.repository_replica.lock().await;
        runtime.prepare_for_replication(now)?;
        runtime.reconcile_ready_repositories(&receiving_repository_ids)?;
    }

    for peer in peers
        .iter()
        .filter(|peer| peer.node_id != state.cluster.node_id)
    {
        let mut caught_up = false;
        for attempt in 0..CATCH_UP_PEER_VERIFICATION_ATTEMPTS {
            match replicate_peer(
                state,
                peer,
                &receiving_repository_ids,
                now,
                ReplicaWork::DeepVerification,
                false,
            )
            .await
            {
                Ok(true) => {
                    caught_up = true;
                    break;
                }
                Ok(false) if catch_up_has_verification_retry(attempt) => continue,
                Ok(false) => break,
                Err(error) => {
                    tracing::debug!(
                        peer = %peer.node_id,
                        error = %error,
                        "history repository catch-up verification failed"
                    );
                    return Ok(false);
                }
            }
        }
        if !caught_up {
            return Ok(false);
        }
    }
    // Signed anti-entropy covers the seven-day detail cache above. One ready repository exports
    // the canonical older tiers below that fixed boundary, so importing every peer cannot create
    // overlapping backfill streams for the same cluster-wide history.
    let Some(tiered_peer) = peers
        .iter()
        .find(|peer| peer.node_id != state.cluster.node_id)
    else {
        return Ok(false);
    };
    if pull_peer_initial_history(state, tiered_peer, &receiving_repository_ids)
        .await?
        .is_none()
    {
        return Ok(false);
    }
    Ok(true)
}

pub(crate) fn catch_up_has_verification_retry(attempt: usize) -> bool {
    attempt.saturating_add(1) < CATCH_UP_PEER_VERIFICATION_ATTEMPTS
}
