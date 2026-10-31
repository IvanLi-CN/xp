use super::*;
use sha2::{Digest as _, Sha256};

pub(super) fn deletion_state_digest(state: &PersistedNodeHistoryCache) -> anyhow::Result<[u8; 32]> {
    struct HashWriter(Sha256);
    impl std::io::Write for HashWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = HashWriter(Sha256::new());
    serde_json::to_writer(
        &mut writer,
        &(
            &state.pending_user_traffic_cleanup,
            &state.pending_node_history_cleanup,
            &state.deleted_users,
            &state.deleted_nodes,
        ),
    )?;
    if !state.deletion_generations.is_empty() {
        serde_json::to_writer(&mut writer, &state.deletion_generations)?;
    }
    if !state.prepared_user_deletions.is_empty() {
        serde_json::to_writer(&mut writer, &state.prepared_user_deletions)?;
    }
    Ok(writer.0.finalize().into())
}

impl NodeHistoryHandle {
    /// Return a bounded prefix of complete deletion intents. A node intent is indivisible:
    /// clearing it after only some of its five schemas have been acknowledged loses data.
    pub(crate) async fn repository_deletion_capture_page(
        &self,
        destination_node_id: &str,
        cluster_id: &str,
    ) -> anyhow::Result<(Option<u64>, Vec<RepositoryHistoryDeletionMarker>)> {
        const MAX_MARKERS: usize = 128;
        const PAGE_BYTES: usize = 128 * 1024;
        let budget = PAGE_BYTES.saturating_sub(
            cluster_id
                .len()
                .saturating_add(destination_node_id.len().saturating_mul(2)),
        );
        let mut groups = BTreeMap::<Vec<u8>, Vec<RepositoryHistoryDeletionMarker>>::new();
        let mut group = Vec::new();
        let mut count = 0usize;
        let mut bytes = 0usize;
        let mut partial = false;
        let mut oversized_key: Option<Vec<u8>> = None;
        let estimate = |marker: &RepositoryHistoryDeletionMarker| {
            marker
                .record_key
                .len()
                .saturating_add(marker.schema_id.len())
                .saturating_add(
                    marker
                        .target_node_id()
                        .unwrap_or(destination_node_id)
                        .len()
                        .saturating_mul(2),
                )
                .saturating_add(256)
        };
        let version = self
            .visit_repository_deletion_markers(destination_node_id, true, |marker| {
                let minimum_bytes = marker
                    .record_key
                    .len()
                    .saturating_add(if marker.target_node_id().is_some() {
                        "path_health.v1".len().max("connections.v1".len())
                    } else {
                        marker.schema_id.len()
                    })
                    .saturating_add(
                        marker
                            .target_node_id()
                            .unwrap_or(destination_node_id)
                            .len()
                            .saturating_mul(2),
                    );
                if minimum_bytes > 192 * 1024 {
                    if oversized_key
                        .as_ref()
                        .is_none_or(|key| marker.record_key < *key)
                    {
                        oversized_key = Some(marker.record_key);
                    }
                    partial = true;
                    return Ok(());
                }
                let complete_group =
                    marker.target_node_id().is_none() || marker.schema_id == "ip_usage.v1";
                group.push(marker);
                if !complete_group {
                    return Ok(());
                }
                let complete = std::mem::take(&mut group);
                count = count.saturating_add(complete.len());
                bytes = bytes.saturating_add(complete.iter().map(&estimate).sum::<usize>());
                groups.insert(complete[0].record_key.clone(), complete);
                while groups.len() > 1 && (count > MAX_MARKERS || bytes > budget) {
                    let (_, removed) = groups.pop_last().expect("nonempty deletion page");
                    count -= removed.len();
                    bytes -= removed.iter().map(&estimate).sum::<usize>();
                    partial = true;
                }
                Ok(())
            })
            .await?;
        debug_assert!(group.is_empty());
        anyhow::ensure!(
            oversized_key.as_ref().is_none_or(|key| groups
                .first_key_value()
                .is_some_and(|(first, _)| first < key)),
            "deletion marker exceeds the signed segment limit"
        );
        Ok((
            (!partial).then_some(version),
            groups.into_values().flatten().collect(),
        ))
    }

    /// Visit the current deletion intent under one read lock without cloning the full set.
    pub(crate) async fn visit_repository_deletion_markers(
        &self,
        destination_node_id: &str,
        require_durable: bool,
        mut visit: impl FnMut(RepositoryHistoryDeletionMarker) -> anyhow::Result<()>,
    ) -> anyhow::Result<u64> {
        let _guard = if require_durable {
            Some(self.persistence_lock.lock().await)
        } else {
            None
        };
        let state = self.inner.read().await;
        if require_durable {
            anyhow::ensure!(
                self.deletion_proof_healthy,
                "source deletion state could not be loaded"
            );
            anyhow::ensure!(
                state.deletion_version != u64::MAX,
                "source deletion version exhausted"
            );
            anyhow::ensure!(
                self.history_storage.mode()
                    == crate::state::history_storage::HistoryStorageMode::Sqlite,
                "stable source deletion export requires SQLite"
            );
            anyhow::ensure!(
                self.durable_deletion_version
                    .load(std::sync::atomic::Ordering::Acquire)
                    == state.deletion_version,
                "source deletion state is not yet durable"
            );
            self.history_storage
                .check_node_history_guard(state.deletion_version, &deletion_state_digest(&state)?)
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }
        let pending_nodes = state.pending_node_history_cleanup.get(destination_node_id);
        let nodes = pending_nodes.into_iter().flat_map(|ids| ids.iter()).chain(
            state
                .deleted_nodes
                .iter()
                .filter(|id| !pending_nodes.is_some_and(|pending| pending.contains(*id))),
        );
        for node in nodes {
            for schema in [
                "runtime.v1",
                "path_health.v1",
                "traffic.v1",
                "connections.v1",
                "ip_usage.v1",
            ] {
                visit(RepositoryHistoryDeletionMarker {
                    schema_id: schema.into(),
                    record_key: format!("node-history:node:{node}:").into_bytes(),
                    generation: state.deletion_generation(true, node),
                    kind: RepositoryHistoryDeletionKind::Node(node.clone()),
                })?;
            }
        }
        let pending_users = state.pending_user_traffic_cleanup.get(destination_node_id);
        let users = pending_users.into_iter().flat_map(|ids| ids.iter()).chain(
            state
                .deleted_users
                .iter()
                .filter(|id| !pending_users.is_some_and(|pending| pending.contains(*id))),
        );
        for user in users {
            visit(RepositoryHistoryDeletionMarker {
                schema_id: "traffic.v1".into(),
                record_key: format!("node-history:user:{user}:").into_bytes(),
                generation: state.deletion_generation(false, user),
                kind: RepositoryHistoryDeletionKind::User(user.clone()),
            })?;
        }
        Ok(state.deletion_version)
    }

    /// Pending cleanup is durable local deletion intent. Repository sources turn these markers
    /// into signed tombstones before publishing later records, and keep retrying until the
    /// repository tombstone ledger has fanned acknowledgements out.
    pub async fn repository_deletion_markers(
        &self,
        destination_node_id: &str,
    ) -> anyhow::Result<Vec<RepositoryHistoryDeletionMarker>> {
        Ok(self
            .repository_deletion_snapshot(destination_node_id)
            .await?
            .1)
    }

    pub(crate) async fn repository_deletion_snapshot(
        &self,
        destination_node_id: &str,
    ) -> anyhow::Result<(u64, Vec<RepositoryHistoryDeletionMarker>)> {
        let mut markers = Vec::new();
        // Capture only durably committed deletion intent. Failure must stop source capture.
        let version = self
            .visit_repository_deletion_markers(destination_node_id, true, |marker| {
                markers.push(marker);
                Ok(())
            })
            .await?;
        markers.sort_by(|left, right| left.record_key.cmp(&right.record_key));
        Ok((version, markers))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn old_ack_preserves_reentered_deletion_after_restart() {
        for node in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("node_history_cache.json");
            let history = NodeHistoryHandle::new(path.clone());
            if node {
                history.clear_node("target").await.unwrap();
            } else {
                history.clear_user("target").await.unwrap();
            }
            let old = history.repository_deletion_markers("local").await.unwrap();
            if node {
                history.clear_node("target").await.unwrap();
            } else {
                history.clear_user("target").await.unwrap();
            }
            for marker in &old {
                history
                    .complete_repository_deletion_marker("local", marker)
                    .await
                    .unwrap();
            }
            drop(history);
            let restored = NodeHistoryHandle::new(path);
            let pending = restored.repository_deletion_markers("local").await.unwrap();
            assert_eq!(pending.len(), if node { 5 } else { 1 });
            assert_ne!(old, pending);
        }
    }

    async fn version(handle: &NodeHistoryHandle) -> u64 {
        handle
            .visit_repository_deletion_markers("local", true, |_| Ok(()))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn deletion_version_covers_every_mutation_and_survives_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("node_history_cache.json");
        let history = NodeHistoryHandle::new(path.clone());
        assert_eq!(version(&history).await, 0);
        history
            .queue_node_history_cleanup("local", "node-a")
            .await
            .unwrap();
        assert_eq!(version(&history).await, 1);
        history
            .queue_node_history_cleanup("local", "node-a")
            .await
            .unwrap();
        assert_eq!(version(&history).await, 2);
        history
            .complete_node_history_cleanup("local", "node-a")
            .await
            .unwrap();
        assert_eq!(version(&history).await, 3);
        history
            .queue_user_traffic_cleanup("local", "user-a")
            .await
            .unwrap();
        assert_eq!(version(&history).await, 4);
        history
            .complete_user_traffic_cleanup("local", "user-a")
            .await
            .unwrap();
        assert_eq!(version(&history).await, 5);
        history.clear_node("node-a").await.unwrap();
        assert_eq!(version(&history).await, 6);
        let marker = history
            .repository_deletion_markers("local")
            .await
            .unwrap()
            .into_iter()
            .find(|marker| marker.target_node_id() == Some("node-a"))
            .unwrap();
        history
            .complete_repository_deletion_marker("local", &marker)
            .await
            .unwrap();
        assert_eq!(version(&history).await, 7);
        history.clear_user("user-a").await.unwrap();
        assert_eq!(version(&history).await, 8);
        let marker = history
            .repository_deletion_markers("local")
            .await
            .unwrap()
            .into_iter()
            .find(|marker| marker.target_node_id().is_none())
            .unwrap();
        history
            .complete_repository_deletion_marker("local", &marker)
            .await
            .unwrap();
        assert_eq!(version(&history).await, 9);
        drop(history);
        assert_eq!(version(&NodeHistoryHandle::new(path)).await, 9);
    }

    #[tokio::test]
    async fn uncommitted_deletion_version_is_never_exported() {
        let directory = tempfile::tempdir().unwrap();
        let history = NodeHistoryHandle::new(directory.path().join("node_history_cache.json"));
        history
            .queue_user_traffic_cleanup("local", "before")
            .await
            .unwrap();
        assert_eq!(version(&history).await, 1);
        let connection =
            rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER reject_deletion_version BEFORE INSERT ON history_snapshots
            WHEN NEW.key = 'node_history' BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
            )
            .unwrap();
        assert!(
            history
                .queue_user_traffic_cleanup("local", "after")
                .await
                .is_err()
        );
        assert_eq!(version(&history).await, 1);
        let markers = history.repository_deletion_markers("local").await.unwrap();
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].record_key, b"node-history:user:before:");
        connection
            .execute_batch("DROP TRIGGER reject_deletion_version")
            .unwrap();
    }

    #[tokio::test]
    async fn concurrent_export_waits_for_deletion_commit() {
        let directory = tempfile::tempdir().unwrap();
        let history = NodeHistoryHandle::new(directory.path().join("node_history_cache.json"));
        let guard = history.persistence_lock.lock().await;
        let writer = history.clone();
        let task =
            tokio::spawn(async move { writer.queue_user_traffic_cleanup("local", "new").await });
        tokio::task::yield_now().await;
        assert!(!task.is_finished());
        assert_eq!(history.inner.read().await.deletion_version, 0);
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(30),
                history.visit_repository_deletion_markers("local", true, |_| Ok(())),
            )
            .await
            .is_err()
        );
        drop(guard);
        task.await.unwrap().unwrap();
        assert_eq!(version(&history).await, 1);
    }

    #[tokio::test]
    async fn independent_stale_handle_cannot_overwrite_a_newer_deletion_state() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("node_history_cache.json");
        let first = NodeHistoryHandle::new(path.clone());
        let stale = NodeHistoryHandle::new(path.clone());
        first
            .queue_user_traffic_cleanup("local", "first")
            .await
            .unwrap();
        assert_eq!(version(&first).await, 1);
        assert!(
            stale
                .visit_repository_deletion_markers("local", true, |_| Ok(()))
                .await
                .is_err()
        );
        assert!(
            stale
                .queue_user_traffic_cleanup("local", "second")
                .await
                .is_err()
        );
        assert!(
            stale
                .visit_repository_deletion_markers("local", true, |_| Ok(()))
                .await
                .is_err()
        );
        drop(first);
        drop(stale);
        let restored = NodeHistoryHandle::new(path);
        assert_eq!(version(&restored).await, 1);
        let markers = restored.repository_deletion_markers("local").await.unwrap();
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].record_key, b"node-history:user:first:");
    }

    #[tokio::test]
    async fn unreadable_history_is_not_replaced_by_a_new_zero_version() {
        let directory = tempfile::tempdir().unwrap();
        let storage = HistoryStorage::open(directory.path());
        storage.write(NODE_HISTORY_KEY, b"unreadable").unwrap();
        let path = directory.path().join("node_history_cache.json");
        let history = NodeHistoryHandle::new(path.clone());
        assert!(
            history
                .queue_user_traffic_cleanup("local", "new")
                .await
                .is_err()
        );
        assert!(
            history
                .visit_repository_deletion_markers("local", true, |_| Ok(()))
                .await
                .is_err()
        );
        assert_eq!(
            storage.read(NODE_HISTORY_KEY).unwrap().unwrap(),
            b"unreadable"
        );
        drop(history);
        let restored = NodeHistoryHandle::new(path);
        assert!(
            restored
                .visit_repository_deletion_markers("local", true, |_| Ok(()))
                .await
                .is_err()
        );
    }
}
