use super::*;
use crate::state::history_repository::HistoryStorage;
use serde::{Deserialize, Serialize};
mod local_guard;

const KEY: &str = "repository_ready_tombstone_barrier_v1";
const MAX_STATE_BYTES: usize = 8 * 1024 * 1024;
// Includes base64 expansion across every Ready peer, not just one page per peer.
const MAX_BUFFER_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct ProtectedStream {
    pub(super) source: String,
    pub(super) epoch: u64,
    pub(super) stream: String,
    pub(super) last: Option<u64>,
}

#[derive(Default, Serialize, Deserialize)]
struct PeerPhase {
    address: String,
    cursor: Option<String>,
    deletions_complete: bool,
    buffer: Option<String>,
    terminal: bool,
    prepared: bool,
}

#[derive(Serialize, Deserialize)]
struct Barrier {
    version: u8,
    epoch: u64,
    #[serde(default)]
    page_digest: String,
    #[serde(default)]
    source_digest: String,
    protected: Vec<ProtectedStream>,
    peers: BTreeMap<String, PeerPhase>,
    next_peer: usize,
    finished: bool,
}

fn storage(state: &AppState) -> anyhow::Result<HistoryStorage> {
    let storage = HistoryStorage::open(&state.config.data_dir);
    anyhow::ensure!(
        storage.is_sqlite(),
        "Ready deletion barrier requires durable SQLite"
    );
    Ok(storage)
}

fn load(state: &AppState) -> anyhow::Result<Option<Barrier>> {
    repository_blocking(|| {
        let storage = storage(state)?;
        let Some(bytes) = storage.read(KEY)? else {
            return Ok(None);
        };
        anyhow::ensure!(
            bytes.len() <= MAX_STATE_BYTES,
            "Ready deletion barrier exceeds limit"
        );
        let barrier: Barrier = serde_json::from_slice(&bytes)?;
        validate_bounds(&barrier)?;
        Ok(Some(barrier))
    })
}

fn save(state: &AppState, barrier: &Barrier) -> anyhow::Result<()> {
    repository_blocking(|| {
        let storage = storage(state)?;
        validate_bounds(barrier)?;
        let bytes = serde_json::to_vec(barrier)?;
        anyhow::ensure!(
            bytes.len() <= MAX_STATE_BYTES,
            "Ready deletion barrier exceeds limit"
        );
        if storage.read(KEY)?.as_deref() == Some(bytes.as_slice()) {
            anyhow::ensure!(storage.is_sqlite(), "Ready deletion barrier storage failed");
            return Ok(());
        }
        storage.write(KEY, &bytes)?;
        anyhow::ensure!(storage.is_sqlite(), "Ready deletion barrier storage failed");
        Ok(())
    })
}

fn validate_bounds(barrier: &Barrier) -> anyhow::Result<()> {
    anyhow::ensure!(
        barrier.version == 1 && barrier.peers.len() <= 4096,
        "invalid Ready deletion barrier"
    );
    let mut total = 0_usize;
    for buffer in barrier
        .peers
        .values()
        .filter_map(|phase| phase.buffer.as_ref())
    {
        anyhow::ensure!(
            buffer.len() <= super::super::MAX_INITIAL_BACKFILL_WIRE_BYTES.div_ceil(3) * 4,
            "Ready buffered page exceeds limit"
        );
        total = total.saturating_add(buffer.len());
    }
    anyhow::ensure!(
        total <= MAX_BUFFER_BYTES,
        "Ready aggregate buffer exceeds limit"
    );
    Ok(())
}

#[cfg(test)]
async fn save_buffer(state: &AppState, barrier: &Barrier) -> anyhow::Result<()> {
    admit_buffer(state, barrier, &mut *state.repository_replica.lock().await)
}

fn admit_buffer(
    state: &AppState,
    barrier: &Barrier,
    runtime: &mut crate::state::history_repository::replica::RepositoryReplicaRuntime,
) -> anyhow::Result<()> {
    validate_bounds(barrier)?;
    let bytes = serde_json::to_vec(barrier)?;
    let capacity = runtime.runtime_capacity()?;
    // Reserve replacement DB pages and their WAL frames plus overhead. Usage includes
    // SQLite and WAL. Keep this runtime lock through admission and the atomic KV replacement.
    let required = (bytes.len() as u64)
        .saturating_mul(2)
        .saturating_add(64 * 1024);
    let reserve =
        crate::state::history_repository::control::HISTORY_REPOSITORY_LOW_SPACE_GUARD_BYTES;
    anyhow::ensure!(
        capacity
            .history_write_availability()
            .allows_history_writes()
            && capacity.quota_bytes().saturating_sub(capacity.used_bytes()) >= required
            && capacity.filesystem_available_bytes() >= required.saturating_add(reserve),
        "Ready buffered page capacity exhausted"
    );
    // Buffer bytes, phase and continuation are one SQLite snapshot transaction. A failed
    // commit keeps the previous page/cursor; a committed-but-reported-failed write is replayable.
    save(state, barrier)
}

async fn persist(state: &AppState, barrier: &mut Barrier) -> anyhow::Result<()> {
    let expected = local_guard::pinned_identity(state).await?;
    let mut runtime = state.repository_replica.lock().await;
    local_guard::refresh_locked(state, barrier, &runtime, &expected)?;
    if barrier.peers.values().any(|phase| phase.buffer.is_some()) {
        admit_buffer(state, barrier, &mut runtime)
    } else {
        save(state, barrier)
    }
}

pub(in crate::http::history_repository::worker::backfill) async fn refresh(
    state: &AppState,
) -> anyhow::Result<()> {
    if let Some(mut barrier) = load(state)? {
        persist(state, &mut barrier).await?;
    }
    Ok(())
}

fn hold_first_records_page(
    phase: &mut PeerPhase,
    bytes: &[u8],
    next_cursor: Option<String>,
    available_buffer: usize,
) {
    let encoded = URL_SAFE_NO_PAD.encode(bytes);
    phase.deletions_complete = true;
    if encoded.len() <= available_buffer {
        phase.buffer = Some(encoded);
        phase.terminal = next_cursor.is_none();
        phase.cursor = next_cursor;
    } else {
        // An authenticated records-only first page proves the empty deletion baseline.
        // No records were applied or ACKed. Restart the records export after local recovery
        // rather than skipping its first page or making the cache cap a convergence barrier.
        phase.buffer = None;
        phase.terminal = false;
        phase.cursor = None;
    }
}

pub(super) fn protected_streams(state: &AppState) -> anyhow::Result<Vec<ProtectedStream>> {
    Ok(load(state)?.map_or_else(Vec::new, |barrier| barrier.protected))
}

pub(super) fn exists(state: &AppState) -> anyhow::Result<bool> {
    Ok(load(state)?.is_some())
}

pub(super) async fn local_pending(state: &AppState) -> bool {
    let runtime = state.repository_replica.lock().await;
    runtime
        .local_history_backfill_inflight_checkpoint()
        .is_some()
        || (!runtime.local_history_backfill_completed()
            && runtime.local_history_backfill_cursor().is_some())
}

pub(super) async fn extend_verified(
    state: &AppState,
    cursor: &crate::history_sync::Cursor,
) -> anyhow::Result<()> {
    let Some(mut barrier) = load(state)? else {
        return Ok(());
    };
    if barrier
        .protected
        .iter()
        .any(|stream| stream.source == cursor.source_node_id() && stream.stream == cursor.stream())
    {
        local_guard::grow(&mut barrier.protected, cursor, None);
    }
    persist(state, &mut barrier).await
}

fn next_phase(cursor: Option<&str>) -> anyhow::Result<Option<String>> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    let value: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(cursor)?)?;
    anyhow::ensure!(
        value["keyset_version"] == 2,
        "Ready deletion export requires v2 cursor"
    );
    let phase = value["phase"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing export phase"))?;
    anyhow::ensure!(
        matches!(phase, "tombstones" | "records"),
        "invalid export phase"
    );
    Ok(Some(phase.to_owned()))
}

pub(crate) async fn before_local(
    state: &AppState,
    now: u64,
) -> anyhow::Result<InitialBackfillProgress> {
    let (ready, peers, missing) = super::super::ready_repository_peers_for_catch_up(state).await?;
    let epoch = state
        .repository_replica
        .lock()
        .await
        .local_history_backfill_collection_state()
        .3;
    let pending = local_pending(state).await;
    let previous = load(state)?.filter(|barrier| barrier.epoch == epoch);
    let mut barrier = if let Some(barrier) = previous {
        barrier
    } else {
        Barrier {
            version: 1,
            epoch,
            page_digest: String::new(),
            source_digest: String::new(),
            protected: Vec::new(),
            peers: BTreeMap::new(),
            next_peer: 0,
            finished: false,
        }
    };
    if barrier.finished && pending {
        // New work needs a fresh deletion baseline, but must not forget older protected streams.
        barrier.peers.clear();
    }
    let targets = peers
        .iter()
        .filter(|peer| peer.node_id != state.cluster.node_id)
        .collect::<Vec<_>>();
    let current = ready
        .iter()
        .filter(|id| *id != &state.cluster.node_id)
        .collect::<BTreeSet<_>>();
    barrier.peers.retain(|id, _| current.contains(id));
    for peer in &targets {
        let phase = barrier.peers.entry(peer.node_id.clone()).or_default();
        if phase.address != peer.public_base_url {
            *phase = PeerPhase {
                address: peer.public_base_url.clone(),
                ..Default::default()
            };
        }
    }
    barrier.finished = false;
    persist(state, &mut barrier).await?;
    if targets.is_empty() {
        return Ok(InitialBackfillProgress::Unavailable);
    }
    let deadline = Instant::now() + Duration::from_secs(13);
    let mut unavailable = missing;
    for _ in 0..MAX_INITIAL_CATCH_UP_PAGES_PER_TICK {
        let Some(index) = (0..targets.len())
            .map(|offset| (barrier.next_peer + offset) % targets.len())
            .find(|index| !barrier.peers[&targets[*index].node_id].deletions_complete)
        else {
            break;
        };
        if Instant::now() >= deadline {
            break;
        }
        let peer = targets[index];
        barrier.next_peer = (index + 1) % targets.len();
        let available_buffer = MAX_BUFFER_BYTES.saturating_sub(
            barrier
                .peers
                .values()
                .filter_map(|phase| phase.buffer.as_ref())
                .map(String::len)
                .sum(),
        );
        let phase = barrier
            .peers
            .get_mut(&peer.node_id)
            .expect("registered phase");
        let endpoint = "/api/admin/_internal/history-repository/initial-backfill";
        let path = phase.cursor.as_ref().map_or_else(
            || format!("{endpoint}?page_size=128"),
            |cursor| format!("{endpoint}?page_size=128&page_cursor={cursor}"),
        );
        let response = tokio::time::timeout_at(
            deadline,
            repository_direct_request_body(
                state,
                peer,
                Method::GET,
                &path,
                Vec::new(),
                super::super::MAX_INITIAL_BACKFILL_WIRE_BYTES,
            ),
        )
        .await;
        let bytes = match response {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(error)) => {
                if super::super::should_restart_peer_backfill(phase.cursor.as_deref(), &error) {
                    phase.cursor = None;
                }
                unavailable = true;
                persist(state, &mut barrier).await?;
                break;
            }
            Err(_) => {
                persist(state, &mut barrier).await?;
                return Ok(InitialBackfillProgress::InProgress);
            }
        };
        let page = super::super::deserialize_initial_backfill_page(&bytes)?;
        super::super::validate_peer_backfill_page(
            &page,
            phase.cursor.as_deref(),
            &state.cluster.cluster_id,
        )?;
        anyhow::ensure!(
            page.records
                .iter()
                .all(|record| record.source_node_id.is_some()),
            "Ready peer returned a historical-source page"
        );
        let next = next_phase(page.next_page_cursor.as_deref())?;
        let has_records = page.records.iter().any(|record| !record.tombstone);
        if has_records {
            anyhow::ensure!(
                phase.cursor.is_none()
                    && page.records.iter().all(|record| !record.tombstone)
                    && next.as_deref() != Some("tombstones"),
                "Ready deletion phase returned records early"
            );
            hold_first_records_page(phase, &bytes, page.next_page_cursor, available_buffer);
        } else {
            let records = page
                .records
                .into_iter()
                .map(|record| {
                    record.into_tiered_backfill_record().and_then(|record| {
                        record.ok_or_else(|| anyhow::anyhow!("missing Ready origin"))
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            let mut receipts = ready.clone();
            if !receipts.contains(&state.cluster.node_id) {
                receipts.push(state.cluster.node_id.clone());
            }
            state
                .repository_replica
                .lock()
                .await
                .import_tiered_backfill_records(records, now, &receipts, &state.cluster.node_id)?;
            phase.deletions_complete = next.as_deref() != Some("tombstones");
            phase.terminal = page.next_page_cursor.is_none();
            phase.cursor = page.next_page_cursor;
        }
        persist(state, &mut barrier).await?;
    }
    Ok(if unavailable {
        InitialBackfillProgress::Unavailable
    } else if barrier.peers.values().all(|phase| phase.deletions_complete) {
        InitialBackfillProgress::Complete
    } else {
        InitialBackfillProgress::InProgress
    })
}

pub(super) async fn after_local(state: &AppState, now: u64) -> anyhow::Result<()> {
    refresh(state).await?;
    let Some(mut barrier) = load(state)? else {
        return Ok(());
    };
    let (ready, _, _) = super::super::ready_repository_peers_for_catch_up(state).await?;
    let ids = barrier.peers.keys().cloned().collect::<Vec<_>>();
    for id in ids {
        let phase = barrier.peers.get_mut(&id).expect("registered phase");
        anyhow::ensure!(phase.deletions_complete, "Ready deletions are incomplete");
        if phase.prepared {
            continue;
        }
        let mut cursor = phase.cursor.clone();
        let mut terminal = phase.terminal;
        let mut runtime = state.repository_replica.lock().await;
        anyhow::ensure!(
            runtime
                .local_history_backfill_inflight_checkpoint()
                .is_none()
                && (runtime.local_history_backfill_completed()
                    || runtime.local_history_backfill_cursor().is_none()),
            "local page changed before Ready records; retry"
        );
        if let Some(buffer) = &phase.buffer {
            let bytes = URL_SAFE_NO_PAD.decode(buffer)?;
            let page = super::super::deserialize_initial_backfill_page(&bytes)?;
            super::super::validate_peer_backfill_page(&page, None, &state.cluster.cluster_id)?;
            let records = page
                .records
                .into_iter()
                .map(|record| {
                    record.into_tiered_backfill_record().and_then(|record| {
                        record.ok_or_else(|| anyhow::anyhow!("missing Ready origin"))
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            runtime.import_tiered_backfill_records(records, now, &ready, &state.cluster.node_id)?;
            terminal = page.next_page_cursor.is_none();
            cursor = page.next_page_cursor;
        }
        runtime.update_initial_peer_backfill_checkpoint(
            &id,
            cursor,
            BTreeMap::new(),
            true,
            terminal,
        )?;
        runtime.update_initial_peer_summary_checkpoint(
            &id,
            None,
            Vec::new(),
            None,
            false,
            false,
        )?;
        drop(runtime);
        // Rows and peer cursor commit first. If this checkpoint write fails, replaying the
        // retained page is idempotent by full origin; never discard it before the cursor commits.
        phase.prepared = true;
        phase.buffer = None;
        persist(state, &mut barrier).await?;
    }
    Ok(())
}

pub(super) async fn finish_records(
    state: &AppState,
    deadline: Instant,
) -> anyhow::Result<InitialBackfillProgress> {
    let Some(mut barrier) = load(state)? else {
        return Ok(InitialBackfillProgress::Complete);
    };
    let (ready, peers, missing) = super::super::ready_repository_peers_for_catch_up(state).await?;
    if missing {
        return Ok(InitialBackfillProgress::Unavailable);
    }
    let deadline = deadline.min(Instant::now() + Duration::from_secs(2));
    let mut progress = InitialBackfillProgress::Complete;
    let mut requests = 0;
    for peer in peers
        .iter()
        .filter(|peer| peer.node_id != state.cluster.node_id)
    {
        if state
            .repository_replica
            .lock()
            .await
            .initial_peer_backfill_checkpoint(&peer.node_id)
            .is_some_and(|checkpoint| checkpoint.completed)
        {
            continue;
        }
        if Instant::now() >= deadline || requests >= MAX_INITIAL_CATCH_UP_PAGES_PER_TICK {
            return Ok(InitialBackfillProgress::InProgress);
        }
        requests += 1;
        let result = tokio::time::timeout_at(
            deadline,
            super::super::pull_peer_initial_history(state, peer, &ready),
        )
        .await;
        progress = progress.combine(match result {
            Ok(result) => result?,
            Err(_) => InitialBackfillProgress::InProgress,
        });
    }
    if progress == InitialBackfillProgress::Complete {
        barrier.finished = true;
        persist(state, &mut barrier).await?;
    }
    Ok(progress)
}

#[cfg(test)]
#[path = "../tests/ready_tombstone_boundaries.rs"]
mod tests;
