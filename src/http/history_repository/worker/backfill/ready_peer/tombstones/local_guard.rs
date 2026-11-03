use super::*;
use crate::{
    history_sync::{Cursor, SignedSegment},
    state::{
        history_repository::{identity::RepositoryNodeIdentity, replica::RepositoryReplicaRuntime},
        history_storage::REPOSITORY_REPLICA_KEY,
    },
};
use sha2::{Digest, Sha256};

// Decode only the facts needed by the guard; do not build a DOM for the full control blob.
#[derive(Deserialize)]
struct LocalSnapshot {
    local_source: SourceBinding,
    #[serde(default)]
    local_history_backfill_inflight: Option<PageBinding>,
    #[serde(default)]
    local_history_backfill_cursor: Option<String>,
    #[serde(default)]
    local_history_backfill_completed: bool,
    #[serde(default)]
    local_history_backfill_epoch: u64,
    #[serde(default)]
    local_history_backfill_stream_state: BTreeMap<String, (u64, Option<[u8; 32]>)>,
    #[serde(default)]
    receiver: Option<ReceiverBinding>,
    #[serde(default)]
    gaps: Vec<GapBinding>,
}
#[derive(Deserialize)]
struct ReceiverBinding {
    streams: Vec<ReceiverStream>,
}
#[derive(Deserialize)]
struct ReceiverStream {
    key: ReceiverKey,
    progress: ReceiverProgress,
}
#[derive(Deserialize)]
struct ReceiverKey {
    source_node_id: String,
    stream: String,
}
#[derive(Deserialize)]
struct ReceiverProgress {
    epoch: u64,
    last_sequence: u64,
    #[serde(default = "verified_chain")]
    hash_chain_verified: bool,
}
fn verified_chain() -> bool {
    true
}
#[derive(Deserialize)]
struct GapBinding {
    source_node_id: String,
    source_epoch: u64,
    stream: String,
}
#[derive(Deserialize)]
struct SourceBinding {
    #[serde(default)]
    node_id: String,
    #[serde(default)]
    epoch: u64,
    #[serde(default)]
    streams: BTreeMap<String, SourceProgress>,
}
#[derive(Serialize, Deserialize)]
struct SourceProgress {
    next_sequence: u64,
    #[serde(default)]
    previous_segment_hash: Option<[u8; 32]>,
}
#[derive(Serialize, Deserialize)]
struct PageBinding {
    #[serde(default)]
    page_cursor: Option<String>,
    completed: bool,
    #[serde(default)]
    segment_ids: Vec<String>,
    #[serde(default)]
    segments: Vec<PageWire>,
    #[serde(default)]
    epoch: u64,
    #[serde(default)]
    stream_state: BTreeMap<String, (u64, Option<[u8; 32]>)>,
    #[serde(default)]
    created_at_unix_seconds: Option<u64>,
}
#[derive(Serialize, Deserialize)]
struct PageWire {
    id: String,
    identity: RepositoryNodeIdentity,
    wire: Vec<u8>,
}

pub(super) async fn pinned_identity(state: &AppState) -> anyhow::Result<RepositoryNodeIdentity> {
    let expected = derived_repository_identity(
        state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone())?,
    )
    .map_err(|error| anyhow::anyhow!("derive protected local identity: {error:?}"))?;
    anyhow::ensure!(
        crate::http::history_repository::identity_is_valid_for_history_replay(state, &expected)
            .await
            .map_err(|error| anyhow::anyhow!("check protected local identity: {error:?}"))?,
        "protected local identity is not pinned"
    );
    Ok(expected)
}

pub(super) fn grow(protected: &mut Vec<ProtectedStream>, cursor: &Cursor, last: Option<u64>) {
    if let Some(stream) = protected.iter_mut().find(|stream| {
        stream.source == cursor.source_node_id()
            && stream.epoch == cursor.source_epoch()
            && stream.stream == cursor.stream()
    }) {
        stream.last = stream.last.max(last);
    } else {
        protected.push(ProtectedStream {
            source: cursor.source_node_id().into(),
            epoch: cursor.source_epoch(),
            stream: cursor.stream().into(),
            last,
        });
    }
}

// The caller holds the runtime lock through refresh, capacity admission and barrier commit.
// Only page/source facts participate in the binding: summary diagnostics cannot invalidate it.
pub(super) fn refresh_locked(
    state: &AppState,
    barrier: &mut Barrier,
    runtime: &RepositoryReplicaRuntime,
    expected: &RepositoryNodeIdentity,
) -> anyhow::Result<()> {
    let bytes = storage(state)?
        .read(REPOSITORY_REPLICA_KEY)?
        .ok_or_else(|| anyhow::anyhow!("missing durable local history checkpoint"))?;
    anyhow::ensure!(
        bytes.len() <= 16 * 1024 * 1024,
        "local checkpoint exceeds limit"
    );
    let snapshot: LocalSnapshot = serde_json::from_slice(&bytes)?;
    let epoch = runtime.local_history_backfill_collection_state().3;
    anyhow::ensure!(
        barrier.epoch == epoch,
        "local source epoch changed during Ready barrier"
    );
    let source = &snapshot.local_source;
    anyhow::ensure!(source.epoch == epoch, "durable local source epoch changed");
    let inflight = snapshot.local_history_backfill_inflight.as_ref();
    let durable_checkpoint = inflight.map(|page| (page.page_cursor.clone(), page.completed));
    anyhow::ensure!(
        durable_checkpoint == runtime.local_history_backfill_inflight_checkpoint()
            && snapshot.local_history_backfill_cursor.as_deref()
                == runtime.local_history_backfill_cursor()
            && snapshot.local_history_backfill_completed
                == runtime.local_history_backfill_completed(),
        "durable local page changed; retry"
    );
    let rows = runtime.local_history_backfill_inflight_segments()?;
    let ids = rows
        .iter()
        .map(|row| hex::encode(Sha256::digest(&row.wire)))
        .collect::<Vec<_>>();
    let durable_ids = inflight.map_or(&[][..], |page| page.segment_ids.as_slice());
    anyhow::ensure!(
        ids == durable_ids,
        "durable local page segment set changed; retry"
    );
    if let Some(page) = inflight.filter(|page| !page.segments.is_empty()) {
        anyhow::ensure!(
            page.segments.len() == rows.len(),
            "durable local page wire set changed"
        );
        for (id, row) in ids.iter().zip(&rows) {
            anyhow::ensure!(
                page.segments.iter().any(|wire| &wire.id == id
                    && wire.identity == row.identity
                    && wire.wire == row.wire),
                "durable local page wire changed; retry"
            );
        }
    }
    for row in rows {
        anyhow::ensure!(
            row.identity == *expected,
            "protected wire identity is not local pinned identity"
        );
        let signed = SignedSegment::from_wire(&row.wire)?;
        signed.verify_identity(expected)?;
        anyhow::ensure!(
            signed.canonical().cluster_id() == state.cluster.cluster_id
                && signed.canonical().first_cursor().source_node_id() == state.cluster.node_id,
            "local protected wire cluster/source mismatch"
        );
        let cursor = signed.canonical().last_cursor();
        // The pending page establishes conflict protection, not a received prefix.
        grow(&mut barrier.protected, cursor, None);
    }
    anyhow::ensure!(
        source.streams.len() <= 4096,
        "local source stream inventory exceeds limit"
    );
    if !source.streams.is_empty() {
        anyhow::ensure!(
            source.node_id.is_empty() || source.node_id == state.cluster.node_id,
            "durable local source identity mismatch"
        );
    }
    for stream in source.streams.keys() {
        let cursor = Cursor::new(&state.cluster.node_id, epoch, stream, 0)?;
        // A live stream name is a conflict guard, not evidence of an imported prefix.
        grow(&mut barrier.protected, &cursor, None);
    }
    // An entire independent page can be queued and ACKed in one worker call. Recover its
    // stream names from the atomic backfill checkpoint even when no in-flight page remains.
    for stream in snapshot.local_history_backfill_stream_state.keys() {
        let cursor = Cursor::new(
            &state.cluster.node_id,
            snapshot.local_history_backfill_epoch,
            stream,
            0,
        )?;
        grow(&mut barrier.protected, &cursor, None);
    }
    if let Some(receiver) = &snapshot.receiver {
        for entry in &receiver.streams {
            let cursor = Cursor::new(
                &entry.key.source_node_id,
                entry.progress.epoch,
                &entry.key.stream,
                entry.progress.last_sequence,
            )?;
            if entry.progress.hash_chain_verified
                && barrier.protected.iter().any(|stream| {
                    stream.source == entry.key.source_node_id && stream.stream == entry.key.stream
                })
                && !snapshot.gaps.iter().any(|gap| {
                    gap.source_node_id == entry.key.source_node_id
                        && gap.source_epoch == entry.progress.epoch
                        && gap.stream == entry.key.stream
                })
            {
                grow(&mut barrier.protected, &cursor, Some(cursor.sequence()));
            }
        }
    }
    barrier.page_digest = hex::encode(Sha256::digest(serde_json::to_vec(&(
        inflight,
        &snapshot.local_history_backfill_cursor,
        snapshot.local_history_backfill_completed,
    ))?));
    barrier.source_digest = hex::encode(Sha256::digest(serde_json::to_vec(&(
        &source.streams,
        snapshot.local_history_backfill_epoch,
        &snapshot.local_history_backfill_stream_state,
    ))?));
    Ok(())
}
