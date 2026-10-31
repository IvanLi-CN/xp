//! Exact original-wire recovery for predecessor in-flight pages.
//! Only the serving peer's existing public summary/repair endpoints are used.
use std::{collections::BTreeSet, time::Duration};

use anyhow::{Context, bail};
use axum::http::Method;
use serde::de::DeserializeOwned;
use sha2::{Digest as _, Sha256};

use crate::{
    control_plane_mesh::{MeshPeerTarget, MeshRequest, PeerDirectPath, peer_target_from_node},
    http::AppState,
    internal_auth::InternalRoute,
    state::history_repository::{
        identity::RepositoryNodeIdentity,
        replica::{RepositoryRepairBatch, RepositoryReplicaSummary},
    },
};

const RETRY_SECONDS: u64 = 5;
const RESPONSE_BYTES: usize = 2 * 1024 * 1024;
// A hung peer must time out before the tick budget, so retry rotation can be saved.
const REQUEST_BUDGET: Duration = Duration::from_secs(5);
const MAX_STEPS_PER_TICK: usize = 8;
// Leave five seconds of the caller's maintenance budget for shutdown and other work.
const TICK_BUDGET: Duration = Duration::from_secs(10);
type RetainedAuthority<'a> = Option<(&'a [(String, [u8; 32])], u64)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyRecoveryOutcome {
    NotLegacy,
    InProgress,
    Complete,
    Unavailable,
}

fn blocking<T>(operation: impl FnOnce() -> T) -> T {
    if tokio::runtime::Handle::current().runtime_flavor()
        == tokio::runtime::RuntimeFlavor::MultiThread
    {
        tokio::task::block_in_place(operation)
    } else {
        operation()
    }
}

fn valid_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn valid_cursor(cursor: &str) -> bool {
    valid_id(
        cursor
            .strip_prefix("t:")
            .or_else(|| cursor.strip_prefix("r:"))
            .unwrap_or(cursor),
    )
}

/// At most eight bounded requests/apply transactions within ten seconds per maintenance tick.
/// Includes registered former/Syncing repositories without changing lifecycle or ACK membership.
/// The local-page worker dispatches predecessor journal pages here before ordinary page replay.
pub(crate) async fn recover_legacy_original_page(
    state: &AppState,
    source_identity: &RepositoryNodeIdentity,
    now: u64,
    ready_repository_ids: &[String],
) -> anyhow::Result<LegacyRecoveryOutcome> {
    recover_legacy_original_page_inner(state, source_identity, now, ready_repository_ids, None)
        .await
}

pub(super) async fn recover_legacy_original_page_with_retained_baseline(
    state: &AppState,
    source_identity: &RepositoryNodeIdentity,
    now: u64,
    ready_repository_ids: &[String],
    peers: &[(String, [u8; 32])],
    generation: u64,
) -> anyhow::Result<LegacyRecoveryOutcome> {
    recover_legacy_original_page_inner(
        state,
        source_identity,
        now,
        ready_repository_ids,
        Some((peers, generation)),
    )
    .await
}

async fn recover_legacy_original_page_inner(
    state: &AppState,
    source_identity: &RepositoryNodeIdentity,
    now: u64,
    ready_repository_ids: &[String],
    authority: RetainedAuthority<'_>,
) -> anyhow::Result<LegacyRecoveryOutcome> {
    let deadline = tokio::time::Instant::now() + TICK_BUDGET;
    for _ in 0..MAX_STEPS_PER_TICK {
        let step = recover_legacy_original_page_step_inner(
            state,
            source_identity,
            now,
            ready_repository_ids,
            authority,
        );
        match tokio::time::timeout_at(deadline, step).await {
            Ok(Ok(LegacyRecoveryOutcome::InProgress)) => tokio::task::yield_now().await,
            Ok(result) => return result,
            // Dropping a network read keeps the durable request/checkpoint available to retry.
            Err(_) => return Ok(LegacyRecoveryOutcome::InProgress),
        }
        if tokio::time::Instant::now() >= deadline {
            break;
        }
    }
    Ok(LegacyRecoveryOutcome::InProgress)
}

#[cfg(test)]
async fn recover_legacy_original_page_step(
    state: &AppState,
    source_identity: &RepositoryNodeIdentity,
    now: u64,
    ready_repository_ids: &[String],
) -> anyhow::Result<LegacyRecoveryOutcome> {
    recover_legacy_original_page_step_inner(state, source_identity, now, ready_repository_ids, None)
        .await
}

async fn recover_legacy_original_page_step_inner(
    state: &AppState,
    source_identity: &RepositoryNodeIdentity,
    now: u64,
    ready_repository_ids: &[String],
    authority: RetainedAuthority<'_>,
) -> anyhow::Result<LegacyRecoveryOutcome> {
    let mut checkpoint = {
        let store = if authority.is_some() {
            Some(state.store.lock().await)
        } else {
            None
        };
        if let (Some(store), Some((peers, _))) = (&store, authority) {
            anyhow::ensure!(
                super::ordering::retained_ready(state, store)?.binding == peers,
                "Ready membership changed before legacy recovery"
            );
        }
        let mut runtime = if store.is_some() {
            let Ok(runtime) = state.repository_replica.try_lock() else {
                return Ok(LegacyRecoveryOutcome::InProgress);
            };
            runtime
        } else {
            state.repository_replica.lock().await
        };
        if runtime
            .local_history_backfill_inflight_checkpoint()
            .is_none()
            || runtime.local_history_backfill_inflight_is_persisted()
        {
            return Ok(LegacyRecoveryOutcome::NotLegacy);
        }
        blocking(|| match authority {
            Some((peers, generation)) => runtime
                .legacy_original_recovery_state_with_retained_baseline(
                    &state.cluster.cluster_id,
                    source_identity,
                    now,
                    peers,
                    generation,
                ),
            None => runtime.legacy_original_recovery_state(
                &state.cluster.cluster_id,
                source_identity,
                now,
            ),
        })?
    };
    if !checkpoint.suffix_from_peer
        && checkpoint
            .targets
            .values()
            .any(|target| target.suffix_next < target.source_end)
    {
        let mut runtime = state.repository_replica.lock().await;
        blocking(|| {
            runtime.legacy_recovery_verify_suffix_step(
                &state.cluster.cluster_id,
                source_identity,
                &checkpoint,
            )
        })?;
        return Ok(LegacyRecoveryOutcome::InProgress);
    }
    if checkpoint.phase == "apply" && !checkpoint.suffix_from_peer {
        let store = if authority.is_some() {
            Some(state.store.lock().await)
        } else {
            None
        };
        if let (Some(store), Some((peers, _))) = (&store, authority) {
            anyhow::ensure!(
                super::ordering::retained_ready(state, store)?.binding == peers,
                "Ready membership changed before legacy receipt"
            );
        }
        let mut runtime = if store.is_some() {
            let Ok(runtime) = state.repository_replica.try_lock() else {
                return Ok(LegacyRecoveryOutcome::InProgress);
            };
            runtime
        } else {
            state.repository_replica.lock().await
        };
        let complete = blocking(|| match authority {
            Some((peers, generation)) => runtime.legacy_recovery_apply_step_with_retained_baseline(
                &state.cluster.cluster_id,
                source_identity,
                &checkpoint,
                now,
                ready_repository_ids,
                &state.cluster.node_id,
                peers,
                generation,
            ),
            None => runtime.legacy_recovery_apply_step(
                &state.cluster.cluster_id,
                source_identity,
                &checkpoint,
                now,
                ready_repository_ids,
                &state.cluster.node_id,
            ),
        })?;
        return Ok(if complete {
            LegacyRecoveryOutcome::Complete
        } else {
            LegacyRecoveryOutcome::InProgress
        });
    }
    if checkpoint.phase != "scan" && !checkpoint.suffix_from_peer {
        bail!("invalid legacy recovery phase");
    }
    if now < checkpoint.retry_after {
        return Ok(LegacyRecoveryOutcome::Unavailable);
    }
    let peer = {
        let store = state.store.lock().await;
        // Bound metadata allocations before cloning peer targets. No arbitrary URL is accepted.
        if store.state().nodes.len() > 4096 {
            bail!("legacy recovery peer inventory exceeds bound");
        }
        let nodes = &store.state().nodes;
        let selected = checkpoint
            .peer_id
            .as_ref()
            .and_then(|id| nodes.get(id))
            .filter(|node| node.node_id != state.cluster.node_id)
            .or_else(|| {
                nodes
                    .iter()
                    .filter(|(id, _)| *id != &state.cluster.node_id)
                    .find(|(id, _)| {
                        checkpoint
                            .after_peer
                            .as_ref()
                            .is_none_or(|after| *id > after)
                    })
                    .map(|(_, node)| node)
            })
            .or_else(|| {
                nodes
                    .iter()
                    .find(|(id, _)| *id != &state.cluster.node_id)
                    .map(|(_, node)| node)
            });
        let Some(node) = selected else {
            return Ok(LegacyRecoveryOutcome::Unavailable);
        };
        // Only public delivery is used; avoid cloning the unrelated endpoint inventory.
        peer_target_from_node(node, &[])
    };
    let url_hash = hex::encode(Sha256::digest(peer.public_base_url.as_bytes()));
    if checkpoint.peer_id.as_deref() != Some(&peer.node_id)
        || checkpoint.peer_url_hash.as_deref() != Some(&url_hash)
    {
        let previous = checkpoint.clone();
        checkpoint.peer_id = Some(peer.node_id.clone());
        checkpoint.peer_url_hash = Some(url_hash);
        checkpoint.cursor = None;
        checkpoint.pending.clear();
        checkpoint.next_cursor = None;
        let mut runtime = state.repository_replica.lock().await;
        blocking(|| runtime.legacy_recovery_update_scan(&previous, &checkpoint))?;
    }
    // Only transport/protocol errors rotate peers. Once a response is committed, a later
    // storage error must propagate instead of attempting a CAS against the old checkpoint.
    let attempt: anyhow::Result<_> = async {
        if checkpoint.pending.is_empty() {
            let path = checkpoint.cursor.as_ref().map_or_else(
                || "/api/admin/_internal/history-repository/summary?deep_verification=false".into(),
                |cursor| {
                    format!(
                        "/api/admin/_internal/history-repository/summary?after_segment_id={cursor}"
                    )
                },
            );
            let summary: RepositoryReplicaSummary =
                request(state, &peer, Method::GET, &path, Vec::new()).await?;
            validate_summary(&summary, checkpoint.cursor.as_deref())?;
            Ok((Some(summary), None))
        } else {
            let ids = checkpoint
                .pending
                .iter()
                .take(64)
                .cloned()
                .collect::<Vec<_>>();
            let body = serde_json::to_vec(&serde_json::json!({"segment_ids":ids}))?;
            let repair: RepositoryRepairBatch = request(
                state,
                &peer,
                Method::POST,
                "/api/admin/_internal/history-repository/repair",
                body,
            )
            .await?;
            validate_repair(&repair, &ids)?;
            Ok((None, Some(repair)))
        }
    }
    .await;
    let (summary, repair) = match attempt {
        Ok(response) => response,
        Err(error) => {
            // Missing history never becomes a gap or an ACK. A later sweep retries it.
            let mut next = checkpoint.clone();
            next.after_peer = next.peer_id.take();
            next.peer_url_hash = None;
            next.cursor = None;
            next.pending.clear();
            next.next_cursor = None;
            next.retry_after = now.saturating_add(RETRY_SECONDS);
            let mut runtime = state.repository_replica.lock().await;
            blocking(|| runtime.legacy_recovery_update_scan(&checkpoint, &next))
                .context("save legacy recovery retry checkpoint")?;
            tracing::debug!(peer=%peer.node_id,error=%error,"legacy prefix recovery deferred");
            return Ok(LegacyRecoveryOutcome::Unavailable);
        }
    };
    if let Some(summary) = summary {
        let mut next = checkpoint.clone();
        if summary.segment_ids.is_empty() {
            next.after_peer = next.peer_id.take();
            next.peer_url_hash = None;
            next.cursor = None;
            next.retry_after = now.saturating_add(RETRY_SECONDS);
        } else {
            next.pending = summary.segment_ids;
            next.next_cursor = summary.next_segment_id;
        }
        let mut runtime = state.repository_replica.lock().await;
        blocking(|| runtime.legacy_recovery_update_scan(&checkpoint, &next))?;
    } else if let Some(repair) = repair {
        let mut runtime = state.repository_replica.lock().await;
        let received = blocking(|| {
            runtime.legacy_recovery_stage_response(
                &state.cluster.cluster_id,
                source_identity,
                &checkpoint,
                &repair,
            )
        });
        let received = match received {
            Ok(received) => received,
            Err(error) => {
                // Invalid source evidence cannot strand recovery on one registered peer.
                // Staging is atomic; this CAS also refuses a stale retry after any commit.
                let mut next = checkpoint.clone();
                next.after_peer = next.peer_id.take();
                next.peer_url_hash = None;
                next.cursor = None;
                next.pending.clear();
                next.next_cursor = None;
                next.retry_after = now.saturating_add(RETRY_SECONDS);
                blocking(|| runtime.legacy_recovery_update_scan(&checkpoint, &next))?;
                tracing::debug!(peer=%peer.node_id,error=%error,"legacy evidence rejected");
                return Ok(LegacyRecoveryOutcome::Unavailable);
            }
        };
        if (received.phase == "scan" || received.suffix_from_peer) && received.pending.is_empty() {
            let mut next = received.clone();
            if let Some(cursor) = next.next_cursor.take() {
                next.cursor = Some(cursor);
            } else {
                next.after_peer = next.peer_id.take();
                next.peer_url_hash = None;
                next.cursor = None;
                next.retry_after = now.saturating_add(RETRY_SECONDS);
            }
            blocking(|| runtime.legacy_recovery_update_scan(&received, &next))?;
        }
    }
    Ok(LegacyRecoveryOutcome::InProgress)
}

fn validate_summary(
    summary: &RepositoryReplicaSummary,
    current: Option<&str>,
) -> anyhow::Result<()> {
    let mut seen = BTreeSet::new();
    if summary.segment_ids.len() > 256
        || summary
            .segment_ids
            .iter()
            .any(|id| !valid_id(id) || !seen.insert(id))
        || summary.next_segment_id.as_deref().is_some_and(|next| {
            !valid_cursor(next)
                || Some(next) == current
                || summary
                    .segment_ids
                    .last()
                    .is_none_or(|id| !next.ends_with(id))
        })
    {
        bail!("invalid or non-advancing legacy summary page");
    }
    Ok(())
}

fn validate_repair(batch: &RepositoryRepairBatch, requested: &[String]) -> anyhow::Result<()> {
    let mut seen = BTreeSet::new();
    if batch.segments.len() > 64
        || batch.unavailable_segment_ids.len() > 64
        || batch.gaps.len() > 64
        || batch.retained_stream_boundaries.len() > 64
        || batch
            .segments
            .iter()
            .map(|row| row.wire.len())
            .sum::<usize>()
            > 256 * 1024
    {
        bail!("legacy repair response exceeds bound");
    }
    for id in batch
        .segments
        .iter()
        .map(|row| hex::encode(Sha256::digest(&row.wire)))
        .chain(batch.unavailable_segment_ids.iter().cloned())
    {
        if !requested.contains(&id) || !seen.insert(id) {
            bail!("unrequested legacy repair ID");
        }
    }
    if seen.is_empty() {
        bail!("legacy repair response did not advance");
    }
    if batch.response_id.as_ref().is_some_and(|id| {
        batch
            .response_id_digest()
            .map_or(true, |actual| *id != actual)
    }) {
        bail!("legacy repair response identity mismatch");
    }
    if batch
        .response_content_digest
        .as_ref()
        .is_some_and(|digest| {
            batch
                .response_content_digest()
                .map_or(true, |actual| *digest != actual)
        })
    {
        bail!("legacy repair response content digest mismatch");
    }
    Ok(())
}

async fn request<T: DeserializeOwned>(
    state: &AppState,
    peer: &MeshPeerTarget,
    method: Method,
    path: &str,
    body: Vec<u8>,
) -> anyhow::Result<T> {
    let url = reqwest::Url::parse(&peer.public_base_url)?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        bail!("legacy recovery requires the registered public HTTPS origin");
    }
    tokio::time::timeout(REQUEST_BUDGET, async {
        let request = MeshRequest {
            method,
            path_and_query: path.into(),
            content_type: (!body.is_empty()).then(|| "application/json".into()),
            body,
            total_budget: REQUEST_BUDGET,
            allow_ambiguous_fallback: false,
            request_id: crate::id::new_ulid_string(),
            route: InternalRoute::MeshV2,
            cluster_id: state.cluster.cluster_id.clone(),
            sender_id: state.cluster.node_id.clone(),
            updates_active_path: false,
        };
        let mut response = state
            .mesh_client
            .send_peer_direct_request(
                peer,
                PeerDirectPath::ApiBaseUrl,
                request,
                state
                    .cluster_ca_key_pem
                    .as_deref()
                    .context("cluster signing key is unavailable")?,
                &state.cluster_ca_pem,
            )
            .await?;
        if !response.status().is_success() {
            bail!("legacy peer rejected request: {}", response.status());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if chunk.len() > RESPONSE_BYTES.saturating_sub(bytes.len()) {
                bail!("legacy peer body exceeds bound");
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).context("decode bounded legacy peer response")
    })
    .await
    .context("legacy peer request timed out")?
}

#[cfg(test)]
#[path = "tests/legacy_recovery_network.rs"]
pub(crate) mod network_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_validation_is_bounded_and_rejects_non_ids() {
        assert!(valid_cursor(&format!("r:{}", "a".repeat(64))));
        assert!(!valid_cursor("../../prefix"));
        assert!(!valid_cursor(&"a".repeat(1024)));
    }
}
