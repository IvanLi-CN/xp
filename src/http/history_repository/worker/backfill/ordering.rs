use super::{AppState, InitialBackfillProgress, RepositoryNodeId};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest as _, Sha256};

type RetainedBaseline = (Vec<(String, [u8; 32])>, u64, Vec<String>);

pub(super) struct RetainedReady {
    pub(super) binding: Vec<(String, [u8; 32])>,
    pub(super) ids: Vec<String>,
    pub(super) targets: Vec<crate::control_plane_mesh::MeshPeerTarget>,
}

pub(super) fn retained_ready(
    state: &AppState,
    store: &crate::state::JsonSnapshotStore,
) -> anyhow::Result<RetainedReady> {
    let membership = store
        .state()
        .repository_membership
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing Ready membership"))?;
    let local_node = RepositoryNodeId::try_from(state.cluster.node_id.clone())?;
    let local_identity = super::derived_repository_identity(state, local_node.clone())
        .map_err(|_| anyhow::anyhow!("derive local history identity"))?;
    let configured_local = membership
        .repository(&local_node)
        .map(|member| member.identity());
    anyhow::ensure!(
        configured_local.is_none_or(|identity| identity == &local_identity),
        "local history identity changed"
    );
    let mut binding = Vec::new();
    let mut ids = Vec::new();
    let mut targets = Vec::new();
    let mut ready_members = membership
        .ready_members()
        .map(|member| member.node_id().as_str().to_owned())
        .collect::<Vec<_>>();
    ready_members.sort();
    if ready_members.is_empty() {
        let mut nodes = store.list_nodes();
        nodes.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        anyhow::ensure!(nodes.len() < 4096, "source inventory exceeds bound");
        let mut sources = Vec::new();
        for node in &nodes {
            let id = RepositoryNodeId::try_from(node.node_id.clone())?;
            let derived = super::derived_repository_identity(state, id.clone())
                .map_err(|_| anyhow::anyhow!("derive source identity"))?;
            if let Some(member) = membership.repository(&id) {
                anyhow::ensure!(member.identity() == &derived, "source identity changed");
            }
            anyhow::ensure!(
                node.node_id == state.cluster.node_id || !node.api_base_url.trim().is_empty(),
                "registered source has no public address"
            );
            sources.push((node.node_id.clone(), derived, node.api_base_url.clone()));
            if node.node_id != state.cluster.node_id {
                targets.push(super::peer_target_from_node(node, &[]));
            }
        }
        if !sources
            .iter()
            .any(|(id, _, _)| id == &state.cluster.node_id)
        {
            sources.push((
                state.cluster.node_id.clone(),
                local_identity.clone(),
                String::new(),
            ));
            sources.sort_by(|a, b| a.0.cmp(&b.0));
        }
        let members = membership
            .members()
            .iter()
            .map(|member| (member.node_id(), member.identity(), member.lifecycle()))
            .collect::<Vec<_>>();
        let inventory = Sha256::digest(serde_json::to_vec(&(
            "no-ready-sources-v1",
            &sources,
            &members,
            &local_identity,
        ))?);
        for (id, _, _) in &sources {
            let value = serde_json::to_vec(&(id, inventory.as_slice()))?;
            binding.push((id.clone(), Sha256::digest(&value).into()));
        }
        binding.sort_by(|a, b| a.0.cmp(&b.0));
        return Ok(RetainedReady {
            binding,
            ids: vec![state.cluster.node_id.clone()],
            targets,
        });
    }
    for member in membership.ready_members() {
        let id = member.node_id().as_str();
        ids.push(id.to_owned());
        if id == state.cluster.node_id {
            continue;
        }
        let node = store
            .get_node(id)
            .ok_or_else(|| anyhow::anyhow!("Ready peer metadata missing"))?;
        let target = super::peer_target_from_node(&node, &[]);
        let value = serde_json::to_vec(&(
            id,
            member.identity(),
            &target.public_base_url,
            &local_identity,
            configured_local,
            &ready_members,
        ))?;
        binding.push((id.to_owned(), Sha256::digest(&value).into()));
        targets.push(target);
    }
    anyhow::ensure!(
        !binding.is_empty() && binding.len() <= 4096,
        "no usable Ready peer"
    );
    binding.sort_by(|a, b| a.0.cmp(&b.0));
    if !ids.contains(&state.cluster.node_id) {
        ids.push(state.cluster.node_id.clone());
    }
    Ok(RetainedReady {
        binding,
        ids,
        targets,
    })
}

pub(super) async fn retained_stale_before_local(
    state: &AppState,
    identity: &crate::state::history_repository::identity::RepositoryNodeIdentity,
    now: u64,
) -> anyhow::Result<Option<RetainedBaseline>> {
    let ready = {
        let store = state.store.lock().await;
        retained_ready(state, &store)?
    };
    let generation = {
        let store = state.store.lock().await;
        anyhow::ensure!(
            retained_ready(state, &store)?.binding == ready.binding,
            "Ready membership changed"
        );
        // Never wait for runtime while holding the membership store: other paths may
        // acquire them in the opposite order. Contention simply retries next tick.
        let Ok(mut runtime) = state.repository_replica.try_lock() else {
            return Ok(None);
        };
        runtime.begin_retained_stale_recovery(
            &state.cluster.cluster_id,
            identity,
            &ready.binding,
            now,
        )?
    };
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    for _ in 0..8 {
        let progress = {
            let runtime = state.repository_replica.lock().await;
            runtime
                .retained_stale_recovery_progress(
                    &state.cluster.cluster_id,
                    identity,
                    &ready.binding,
                )?
                .ok_or_else(|| anyhow::anyhow!("stale generation missing"))?
        };
        if progress.1 {
            return Ok(Some((ready.binding, generation, ready.ids)));
        }
        let Some((id, cursor, _)) = progress.2.into_iter().find(|(_, _, done)| !done) else {
            let store = state.store.lock().await;
            anyhow::ensure!(
                retained_ready(state, &store)?.binding == ready.binding,
                "Ready membership changed"
            );
            let Ok(mut runtime) = state.repository_replica.try_lock() else {
                return Ok(None);
            };
            let current = runtime
                .retained_stale_recovery_progress(
                    &state.cluster.cluster_id,
                    identity,
                    &ready.binding,
                )?
                .ok_or_else(|| anyhow::anyhow!("stale generation missing"))?;
            if current.0 != generation {
                return Ok(None);
            }
            if current.1 {
                return Ok(Some((ready.binding, generation, ready.ids)));
            }
            if current.2.iter().any(|(_, _, done)| !done) {
                return Ok(None);
            }
            runtime.seal_retained_stale_recovery(
                &state.cluster.cluster_id,
                identity,
                &ready.binding,
                generation,
                now,
            )?;
            return Ok(Some((ready.binding, generation, ready.ids)));
        };
        let target = ready
            .targets
            .iter()
            .find(|peer| peer.node_id == id)
            .ok_or_else(|| anyhow::anyhow!("Ready peer disappeared"))?;
        let endpoint = "/api/admin/_internal/history-repository/initial-backfill";
        let path = cursor.as_ref().map_or_else(
            || format!("{endpoint}?page_size=128"),
            |cursor| format!("{endpoint}?page_size=128&page_cursor={cursor}"),
        );
        let response = tokio::time::timeout_at(
            deadline,
            super::repository_direct_request_body(
                state,
                target,
                super::Method::GET,
                &path,
                Vec::new(),
                super::MAX_INITIAL_BACKFILL_WIRE_BYTES,
            ),
        )
        .await;
        let bytes = match response {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(error)) => {
                if super::should_restart_peer_backfill(cursor.as_deref(), &error) {
                    let store = state.store.lock().await;
                    anyhow::ensure!(
                        retained_ready(state, &store)?.binding == ready.binding,
                        "Ready membership changed"
                    );
                    let Ok(mut runtime) = state.repository_replica.try_lock() else {
                        return Ok(None);
                    };
                    let current = runtime
                        .retained_stale_recovery_progress(
                            &state.cluster.cluster_id,
                            identity,
                            &ready.binding,
                        )?
                        .ok_or_else(|| anyhow::anyhow!("stale generation missing"))?;
                    if current.0 != generation
                        || current.1
                        || !current.2.iter().any(|(peer, previous, done)| {
                            peer == &id && !done && previous == &cursor
                        })
                    {
                        return Ok(None);
                    }
                    runtime.restart_retained_stale_peer(
                        &state.cluster.cluster_id,
                        identity,
                        &ready.binding,
                        generation,
                        &id,
                        cursor.as_deref(),
                    )?;
                }
                return Ok(None);
            }
            Err(_) => return Ok(None),
        };
        let page = super::deserialize_initial_backfill_page(&bytes)?;
        super::validate_peer_backfill_page(&page, cursor.as_deref(), &state.cluster.cluster_id)?;
        anyhow::ensure!(
            page.records.iter().all(|row| row.source_node_id.is_some()),
            "Ready export lacks original cursor"
        );
        let phase = page
            .next_page_cursor
            .as_deref()
            .map(|cursor| {
                let decoded = URL_SAFE_NO_PAD.decode(cursor)?;
                let value: serde_json::Value = serde_json::from_slice(&decoded)?;
                anyhow::ensure!(
                    value["keyset_version"] == 2,
                    "Ready cursor is not keyset v2"
                );
                Ok::<_, anyhow::Error>(value["phase"].as_str().map(str::to_owned))
            })
            .transpose()?
            .flatten();
        let records_page = page.records.iter().any(|row| !row.tombstone);
        if records_page {
            anyhow::ensure!(
                cursor.is_none()
                    && page.records.iter().all(|row| !row.tombstone)
                    && phase.as_deref() != Some("tombstones"),
                "Ready export interleaved records and deletions"
            );
        }
        let done = phase.as_deref() != Some("tombstones");
        let records = if records_page {
            Vec::new()
        } else {
            page.records
                .into_iter()
                .map(|row| {
                    row.into_tiered_backfill_record()
                        .and_then(|row| row.ok_or_else(|| anyhow::anyhow!("missing Ready cursor")))
                })
                .collect::<anyhow::Result<Vec<_>>>()?
        };
        let store = state.store.lock().await;
        anyhow::ensure!(
            retained_ready(state, &store)?.binding == ready.binding,
            "Ready membership changed"
        );
        let Ok(mut runtime) = state.repository_replica.try_lock() else {
            return Ok(None);
        };
        let current = runtime
            .retained_stale_recovery_progress(&state.cluster.cluster_id, identity, &ready.binding)?
            .ok_or_else(|| anyhow::anyhow!("stale generation missing"))?;
        if current.0 != generation
            || current.1
            || !current
                .2
                .iter()
                .any(|(peer, previous, done)| peer == &id && !done && previous == &cursor)
        {
            return Ok(None);
        }
        runtime.import_retained_stale_deletions(
            &state.cluster.cluster_id,
            identity,
            &ready.binding,
            generation,
            &id,
            cursor.as_deref(),
            page.next_page_cursor.as_deref(),
            done,
            records,
            now,
            &state.cluster.node_id,
        )?;
    }
    Ok(None)
}

pub(super) async fn retained_stale_sources_before_local(
    state: &AppState,
    identity: &crate::state::history_repository::identity::RepositoryNodeIdentity,
    now: u64,
) -> anyhow::Result<Option<RetainedBaseline>> {
    let sources = {
        let store = state.store.lock().await;
        retained_ready(state, &store)?
    };
    let generation = {
        let store = state.store.lock().await;
        anyhow::ensure!(
            retained_ready(state, &store)?.binding == sources.binding,
            "source membership changed"
        );
        let Ok(mut runtime) = state.repository_replica.try_lock() else {
            return Ok(None);
        };
        runtime.begin_retained_stale_recovery(
            &state.cluster.cluster_id,
            identity,
            &sources.binding,
            now,
        )?
    };
    let signing_key = super::derived_repository_signing_key(state, identity.node_id().as_str())
        .map_err(|_| anyhow::anyhow!("derive recovery signing key"))?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    for _ in 0..8 {
        let progress = state
            .repository_replica
            .lock()
            .await
            .retained_stale_recovery_progress(
                &state.cluster.cluster_id,
                identity,
                &sources.binding,
            )?
            .ok_or_else(|| anyhow::anyhow!("stale generation missing"))?;
        if progress.0 != generation {
            return Ok(None);
        }
        if progress.1 {
            return Ok(Some((sources.binding, generation, sources.ids)));
        }
        let Some((id, cursor, _)) = progress.2.into_iter().find(|(_, _, done)| !done) else {
            let store = state.store.lock().await;
            anyhow::ensure!(
                retained_ready(state, &store)?.binding == sources.binding,
                "source membership changed"
            );
            let Ok(mut runtime) = state.repository_replica.try_lock() else {
                return Ok(None);
            };
            runtime.seal_retained_stale_recovery(
                &state.cluster.cluster_id,
                identity,
                &sources.binding,
                generation,
                now,
            )?;
            return Ok(Some((sources.binding, generation, sources.ids)));
        };
        let response = if id == state.cluster.node_id {
            match tokio::time::timeout_at(
                deadline,
                super::source_deletions::page(state, cursor.as_deref(), 128),
            )
            .await
            {
                Ok(Ok(page)) => Some(page),
                Ok(Err(_)) if cursor.is_some() => {
                    let store = state.store.lock().await;
                    anyhow::ensure!(
                        retained_ready(state, &store)?.binding == sources.binding,
                        "source membership changed"
                    );
                    let Ok(mut runtime) = state.repository_replica.try_lock() else {
                        return Ok(None);
                    };
                    runtime.restart_retained_stale_peer(
                        &state.cluster.cluster_id,
                        identity,
                        &sources.binding,
                        generation,
                        &id,
                        cursor.as_deref(),
                    )?;
                    return Ok(None);
                }
                _ => None,
            }
        } else {
            let target = sources
                .targets
                .iter()
                .find(|target| target.node_id == id)
                .ok_or_else(|| anyhow::anyhow!("registered source disappeared"))?;
            let endpoint = "/api/admin/_internal/history-repository/initial-backfill";
            let path = cursor.as_ref().map_or_else(
                || format!("{endpoint}?page_size=128&stable_deletions=true"),
                |cursor| {
                    format!("{endpoint}?page_size=128&stable_deletions=true&page_cursor={cursor}")
                },
            );
            match tokio::time::timeout_at(
                deadline,
                super::repository_direct_request_body(
                    state,
                    target,
                    super::Method::GET,
                    &path,
                    Vec::new(),
                    super::MAX_INITIAL_BACKFILL_WIRE_BYTES,
                ),
            )
            .await
            {
                Ok(Ok(bytes)) => Some(super::deserialize_initial_backfill_page(&bytes)?),
                Ok(Err(error))
                    if super::should_restart_peer_backfill(cursor.as_deref(), &error) =>
                {
                    let store = state.store.lock().await;
                    anyhow::ensure!(
                        retained_ready(state, &store)?.binding == sources.binding,
                        "source membership changed"
                    );
                    let Ok(mut runtime) = state.repository_replica.try_lock() else {
                        return Ok(None);
                    };
                    runtime.restart_retained_stale_peer(
                        &state.cluster.cluster_id,
                        identity,
                        &sources.binding,
                        generation,
                        &id,
                        cursor.as_deref(),
                    )?;
                    return Ok(None);
                }
                _ => None,
            }
        };
        let Some(page) = response else {
            return Ok(None);
        };
        validate_initial_source_page(&page, cursor.as_deref(), &state.cluster.cluster_id)?;
        anyhow::ensure!(
            page.records
                .iter()
                .all(|row| row.tombstone && row.source_node_id.is_none()),
            "source deletion page has unexpected records"
        );
        let snapshot = page
            .deletion_snapshot
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("source lacks stable deletion snapshot"))?;
        let records = page
            .records
            .into_iter()
            .map(|row| row.into_sync_record())
            .collect::<anyhow::Result<Vec<_>>>()?;
        anyhow::ensure!(
            records
                .iter()
                .all(|(_, record)| record.record_key().ends_with(b":")),
            "source deletion page contains unsupported exact marker"
        );
        let store = state.store.lock().await;
        anyhow::ensure!(
            retained_ready(state, &store)?.binding == sources.binding,
            "source membership changed"
        );
        let Ok(mut runtime) = state.repository_replica.try_lock() else {
            return Ok(None);
        };
        runtime.import_retained_stale_source_deletions(
            &state.cluster.cluster_id,
            identity,
            &signing_key,
            &sources.binding,
            generation,
            &id,
            cursor.as_deref(),
            page.next_page_cursor.as_deref(),
            snapshot,
            records,
            now,
        )?;
    }
    Ok(None)
}

pub(super) fn is_transient_source_rejection(error: &super::RepositoryDirectError) -> bool {
    // The direct transport exposes application status in this fixed error envelope.
    let message = error.to_string();
    message
        .strip_prefix("repository peer rejected request with ")
        .is_some_and(|status| {
            status.starts_with('5') || status.starts_with("408 ") || status.starts_with("429 ")
        })
}

pub(super) fn validate_initial_source_page(
    page: &super::RepositoryInitialBackfillPage,
    cursor: Option<&str>,
    cluster: &str,
) -> anyhow::Result<()> {
    if page
        .records
        .iter()
        .all(|record| record.source_node_id.is_none())
        && serde_json::to_vec(&page.records)?.len() > super::MAX_INITIAL_BACKFILL_PAGE_BYTES
    {
        anyhow::bail!("peer history backfill page exceeds byte limit");
    }
    let mut sorted = page.clone();
    for record in &mut sorted.records {
        if record.source_node_id.is_none() && record.tombstone {
            // Historical exports sort deletion intent at zero, independently of wire time.
            // Validate that same key while retaining the original observation for application.
            record.observed_at_unix_seconds = 0;
        }
    }
    super::validate_peer_backfill_page(&sorted, cursor, cluster)
}

pub(crate) async fn advance_initial_catch_up(
    state: &AppState,
    now: u64,
    catch_up_completed: bool,
    has_ready_peers: bool,
) -> anyhow::Result<InitialBackfillProgress> {
    let local_inflight = super::super::source::local_history_backfill_inflight(state).await;
    let local_pending = local_inflight || {
        let runtime = state.repository_replica.lock().await;
        !runtime.local_history_backfill_completed()
            && runtime.local_history_backfill_cursor().is_some()
    };
    if !should_run_initial_catch_up(catch_up_completed) && !local_pending {
        return Ok(InitialBackfillProgress::Complete);
    }
    if catch_up_completed && local_pending {
        // A restart can expose unfinished durable work after Raft recorded completion.
        // Start a new stability window only after its terminal local page is committed.
        let node_id = RepositoryNodeId::try_from(state.cluster.node_id.clone())?;
        super::super::apply_local_catch_up_result(state, &node_id, now, false).await?;
    }
    if local_inflight {
        let node = RepositoryNodeId::try_from(state.cluster.node_id.clone())?;
        let identity = super::derived_repository_identity(state, node)
            .map_err(|_| anyhow::anyhow!("derive stale recovery identity"))?;
        let needed = state
            .repository_replica
            .lock()
            .await
            .legacy_page_needs_retained_stale_recovery(&state.cluster.cluster_id, &identity, now)?;
        if needed {
            if !has_ready_peers
                && state
                    .store
                    .lock()
                    .await
                    .state()
                    .repository_membership
                    .is_none()
            {
                return Ok(InitialBackfillProgress::Unavailable);
            }
            anyhow::ensure!(
                super::super::super::identity_is_valid_for_history_replay(state, &identity)
                    .await
                    .map_err(|_| anyhow::anyhow!("check local history identity"))?,
                "local history identity is not pinned"
            );
            let baseline = if has_ready_peers {
                retained_stale_before_local(state, &identity, now).await?
            } else {
                retained_stale_sources_before_local(state, &identity, now).await?
            };
            let Some((peers, generation, ready)) = baseline else {
                return Ok(InitialBackfillProgress::InProgress);
            };
            let outcome =
                super::legacy_recovery::recover_legacy_original_page_with_retained_baseline(
                    state, &identity, now, &ready, &peers, generation,
                )
                .await?;
            return Ok(match outcome {
                super::legacy_recovery::LegacyRecoveryOutcome::Unavailable => {
                    InitialBackfillProgress::Unavailable
                }
                _ => InitialBackfillProgress::InProgress,
            });
        }
    }
    if has_ready_peers && local_pending {
        let progress = super::ready_peer::tombstones::before_local(state, now).await?;
        if progress != InitialBackfillProgress::Complete {
            return Ok(progress);
        }
        // Peer deletion completion is distinct from full summary/repair convergence.
        super::ready_peer::tombstones::refresh(state).await?;
        super::backfill_initial_repository_from_local_history(state, now).await?;
        super::ready_peer::tombstones::refresh(state).await?;
        return Ok(InitialBackfillProgress::InProgress);
    }
    catch_up_before_pending_local(
        local_pending,
        has_ready_peers,
        || super::catch_up_against_ready_repositories(state, now),
        || super::backfill_initial_repository_from_local_history(state, now),
    )
    .await
}

pub(crate) async fn catch_up_before_pending_local<P, L, PFut, LFut>(
    local_pending: bool,
    has_ready_peers: bool,
    peer_catch_up: P,
    local_backfill: L,
) -> anyhow::Result<InitialBackfillProgress>
where
    P: FnOnce() -> PFut,
    L: FnOnce() -> LFut,
    PFut: std::future::Future<Output = anyhow::Result<InitialBackfillProgress>>,
    LFut: std::future::Future<Output = anyhow::Result<InitialBackfillProgress>>,
{
    if !has_ready_peers {
        return local_backfill().await;
    }
    let peer_progress = peer_catch_up().await?;
    if peer_progress == InitialBackfillProgress::Complete && local_pending {
        local_backfill().await
    } else {
        Ok(peer_progress)
    }
}

pub(crate) fn should_run_initial_catch_up(catch_up_completed: bool) -> bool {
    !catch_up_completed
}
