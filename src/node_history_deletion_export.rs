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
    Ok(writer.0.finalize().into())
}

impl NodeHistoryHandle {
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
    ) -> Vec<RepositoryHistoryDeletionMarker> {
        let state = self.inner.read().await;
        let node_ids = state
            .pending_node_history_cleanup
            .get(destination_node_id)
            .into_iter()
            .flat_map(|nodes| nodes.iter())
            .chain(state.deleted_nodes.iter())
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut markers = node_ids
            .into_iter()
            .flat_map(|node_id| {
                [
                    "runtime.v1",
                    "path_health.v1",
                    "traffic.v1",
                    "connections.v1",
                    "ip_usage.v1",
                ]
                .into_iter()
                .map(move |schema_id| RepositoryHistoryDeletionMarker {
                    schema_id: schema_id.to_owned(),
                    record_key: format!("node-history:node:{node_id}:").into_bytes(),
                    kind: RepositoryHistoryDeletionKind::Node(node_id.clone()),
                })
            })
            .collect::<Vec<_>>();
        markers.extend(
            state
                .pending_user_traffic_cleanup
                .get(destination_node_id)
                .into_iter()
                .flat_map(|users| users.iter())
                .chain(state.deleted_users.iter())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(|user_id| RepositoryHistoryDeletionMarker {
                    schema_id: "traffic.v1".to_owned(),
                    record_key: format!("node-history:user:{user_id}:").into_bytes(),
                    kind: RepositoryHistoryDeletionKind::User(user_id.to_string()),
                }),
        );
        markers.sort_by(|left, right| left.record_key.cmp(&right.record_key));
        markers
    }
    pub async fn complete_repository_deletion_marker(
        &self,
        destination_node_id: &str,
        marker: &RepositoryHistoryDeletionMarker,
    ) {
        match &marker.kind {
            RepositoryHistoryDeletionKind::Node(node_id) => {
                self.complete_node_history_cleanup(destination_node_id, node_id)
                    .await;
                let changed = {
                    let mut state = self.inner.write().await;
                    let changed = state.deleted_nodes.remove(node_id);
                    if changed {
                        state.bump_deletion_version();
                    }
                    changed
                };
                if changed {
                    self.persist().await;
                }
            }
            RepositoryHistoryDeletionKind::User(user_id) => {
                self.complete_user_traffic_cleanup(destination_node_id, user_id)
                    .await;
                let changed = {
                    let mut state = self.inner.write().await;
                    let changed = state.deleted_users.remove(user_id);
                    if changed {
                        state.bump_deletion_version();
                    }
                    changed
                };
                if changed {
                    self.persist().await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        history.queue_node_history_cleanup("local", "node-a").await;
        assert_eq!(version(&history).await, 1);
        history.queue_node_history_cleanup("local", "node-a").await;
        assert_eq!(version(&history).await, 1);
        history
            .complete_node_history_cleanup("local", "node-a")
            .await;
        assert_eq!(version(&history).await, 2);
        history.queue_user_traffic_cleanup("local", "user-a").await;
        assert_eq!(version(&history).await, 3);
        history
            .complete_user_traffic_cleanup("local", "user-a")
            .await;
        assert_eq!(version(&history).await, 4);
        history.clear_node("node-a").await;
        assert_eq!(version(&history).await, 5);
        let marker = history
            .repository_deletion_markers("local")
            .await
            .into_iter()
            .find(|marker| marker.target_node_id() == Some("node-a"))
            .unwrap();
        history
            .complete_repository_deletion_marker("local", &marker)
            .await;
        assert_eq!(version(&history).await, 6);
        history.clear_user("user-a").await;
        assert_eq!(version(&history).await, 7);
        let marker = history
            .repository_deletion_markers("local")
            .await
            .into_iter()
            .find(|marker| marker.target_node_id().is_none())
            .unwrap();
        history
            .complete_repository_deletion_marker("local", &marker)
            .await;
        assert_eq!(version(&history).await, 8);
        drop(history);
        assert_eq!(version(&NodeHistoryHandle::new(path)).await, 8);
    }

    #[tokio::test]
    async fn uncommitted_deletion_version_is_never_exported() {
        let directory = tempfile::tempdir().unwrap();
        let history = NodeHistoryHandle::new(directory.path().join("node_history_cache.json"));
        history.queue_user_traffic_cleanup("local", "before").await;
        assert_eq!(version(&history).await, 1);
        let connection =
            rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER reject_deletion_version BEFORE INSERT ON history_snapshots
            WHEN NEW.key = 'node_history' BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
            )
            .unwrap();
        history.queue_user_traffic_cleanup("local", "after").await;
        assert!(
            history
                .visit_repository_deletion_markers("local", true, |_| Ok(()))
                .await
                .is_err()
        );
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
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while history.inner.read().await.deletion_version == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(30),
                history.visit_repository_deletion_markers("local", true, |_| Ok(())),
            )
            .await
            .is_err()
        );
        drop(guard);
        task.await.unwrap();
        assert_eq!(version(&history).await, 1);
    }

    #[tokio::test]
    async fn independent_stale_handle_cannot_overwrite_a_newer_deletion_state() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("node_history_cache.json");
        let first = NodeHistoryHandle::new(path.clone());
        let stale = NodeHistoryHandle::new(path.clone());
        first.queue_user_traffic_cleanup("local", "first").await;
        assert_eq!(version(&first).await, 1);
        assert!(
            stale
                .visit_repository_deletion_markers("local", true, |_| Ok(()))
                .await
                .is_err()
        );
        stale.queue_user_traffic_cleanup("local", "second").await;
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
        let markers = restored.repository_deletion_markers("local").await;
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
        history.queue_user_traffic_cleanup("local", "new").await;
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
