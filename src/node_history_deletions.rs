use super::*;

impl PersistedNodeHistoryCache {
    pub(super) fn deletion_generation(&self, node: bool, id: &str) -> u64 {
        self.deletion_generations
            .get(&generation_key(node, id))
            .copied()
            .unwrap_or(0)
    }

    fn remove_cleanup(&mut self, destination: &str, id: &str, node: bool) -> bool {
        let pending = if node {
            &mut self.pending_node_history_cleanup
        } else {
            &mut self.pending_user_traffic_cleanup
        };
        let Some(ids) = pending.get_mut(destination) else {
            return false;
        };
        let changed = ids.remove(id);
        if ids.is_empty() {
            pending.remove(destination);
        }
        changed
    }

    fn retire_generation(&mut self, node: bool, id: &str) {
        let (deleted, pending) = if node {
            (&self.deleted_nodes, &self.pending_node_history_cleanup)
        } else {
            (&self.deleted_users, &self.pending_user_traffic_cleanup)
        };
        if !deleted.contains(id) && !pending.values().any(|ids| ids.contains(id)) {
            self.deletion_generations.remove(&generation_key(node, id));
        }
    }

    fn prune_generations(&mut self) {
        self.deletion_generations.retain(|key, _| {
            let Some((kind, id)) = key.split_once(':') else {
                return true;
            };
            match kind {
                "node" => {
                    self.deleted_nodes.contains(id)
                        || self
                            .pending_node_history_cleanup
                            .values()
                            .any(|ids| ids.contains(id))
                }
                "user" => {
                    self.deleted_users.contains(id)
                        || self
                            .pending_user_traffic_cleanup
                            .values()
                            .any(|ids| ids.contains(id))
                }
                _ => true,
            }
        });
    }
}

fn generation_key(node: bool, id: &str) -> String {
    format!("{}:{id}", if node { "node" } else { "user" })
}

impl NodeHistoryHandle {
    // Serialize mutation, persistence and the durable version. A failed write restores the
    // previous in-memory intent before releasing the lock or returning to an RPC caller.
    async fn change_deletion(
        &self,
        change: impl FnOnce(&mut PersistedNodeHistoryCache, u64) -> bool,
    ) -> anyhow::Result<()> {
        let _guard = self.persistence_lock.lock().await;
        anyhow::ensure!(
            self.deletion_proof_healthy,
            "source deletion state could not be loaded"
        );
        let mut state = self.inner.write().await;
        if self.history_storage.mode() == crate::state::history_storage::HistoryStorageMode::Sqlite
        {
            self.history_storage
                .check_node_history_guard(
                    state.deletion_version,
                    &deletion_export::deletion_state_digest(&state)?,
                )
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }
        anyhow::ensure!(
            state.deletion_version < i64::MAX as u64,
            "source deletion version exhausted"
        );
        let previous = state.clone();
        let generation = state.deletion_version + 1;
        if !change(&mut state, generation) {
            return Ok(());
        }
        state.bump_deletion_version();
        if let Err(error) = persist_history_cache(&self.history_storage, &state) {
            *state = previous;
            return Err(error);
        }
        self.durable_deletion_version
            .store(state.deletion_version, std::sync::atomic::Ordering::Release);
        Ok(())
    }

    pub async fn clear_node(&self, node_id: &str) -> anyhow::Result<()> {
        self.change_deletion(|state, generation| {
            state.deleted_nodes.insert(node_id.into());
            state
                .deletion_generations
                .insert(generation_key(true, node_id), generation);
            state.nodes.remove(node_id);
            state.pending_user_traffic_cleanup.remove(node_id);
            state.pending_node_history_cleanup.remove(node_id);
            for targets in state.pending_node_history_cleanup.values_mut() {
                targets.remove(node_id);
            }
            state
                .pending_node_history_cleanup
                .retain(|_, targets| !targets.is_empty());
            state.prune_generations();
            true
        })
        .await
    }

    pub async fn clear_user(&self, user_id: &str) -> anyhow::Result<()> {
        self.change_deletion(|state, generation| {
            state.deleted_users.insert(user_id.into());
            state
                .deletion_generations
                .insert(generation_key(false, user_id), generation);
            for record in state.nodes.values_mut() {
                record.user_traffic.remove(user_id);
                record.user_traffic_users.remove(user_id);
            }
            true
        })
        .await
    }

    pub async fn queue_user_traffic_cleanup(
        &self,
        destination: &str,
        user: &str,
    ) -> anyhow::Result<()> {
        self.queue_cleanup(destination, user, false).await
    }

    pub async fn queue_node_history_cleanup(
        &self,
        destination: &str,
        node: &str,
    ) -> anyhow::Result<()> {
        self.queue_cleanup(destination, node, true).await
    }

    async fn queue_cleanup(&self, destination: &str, id: &str, node: bool) -> anyhow::Result<()> {
        self.change_deletion(|state, generation| {
            let pending = if node {
                &mut state.pending_node_history_cleanup
            } else {
                &mut state.pending_user_traffic_cleanup
            };
            pending
                .entry(destination.into())
                .or_default()
                .insert(id.into());
            state
                .deletion_generations
                .insert(generation_key(node, id), generation);
            true
        })
        .await
    }

    pub(super) async fn pending_cleanup_generations(
        &self,
        destination: &str,
        node: bool,
    ) -> Vec<(String, u64)> {
        let state = self.inner.read().await;
        let pending = if node {
            &state.pending_node_history_cleanup
        } else {
            &state.pending_user_traffic_cleanup
        };
        pending
            .get(destination)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .map(|id| (id.clone(), state.deletion_generation(node, id)))
            .collect()
    }

    #[cfg(test)]
    pub(super) async fn pending_user_traffic_cleanup(&self, destination: &str) -> Vec<String> {
        self.pending_cleanup_generations(destination, false)
            .await
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    }

    #[cfg(test)]
    pub(super) async fn pending_node_history_cleanup(&self, destination: &str) -> Vec<String> {
        self.pending_cleanup_generations(destination, true)
            .await
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    }

    pub async fn complete_user_traffic_cleanup(
        &self,
        destination: &str,
        user: &str,
    ) -> anyhow::Result<()> {
        let generation = self.inner.read().await.deletion_generation(false, user);
        self.complete_cleanup_generation(destination, user, false, generation)
            .await
    }

    pub async fn complete_node_history_cleanup(
        &self,
        destination: &str,
        node: &str,
    ) -> anyhow::Result<()> {
        let generation = self.inner.read().await.deletion_generation(true, node);
        self.complete_cleanup_generation(destination, node, true, generation)
            .await
    }

    pub(super) async fn complete_cleanup_generation(
        &self,
        destination: &str,
        id: &str,
        node: bool,
        generation: u64,
    ) -> anyhow::Result<()> {
        self.change_deletion(|state, _| {
            if state.deletion_generation(node, id) != generation {
                return false;
            }
            let changed = state.remove_cleanup(destination, id, node);
            if changed {
                state.retire_generation(node, id);
            }
            changed
        })
        .await
    }

    pub async fn complete_repository_deletion_marker(
        &self,
        destination: &str,
        marker: &RepositoryHistoryDeletionMarker,
    ) -> anyhow::Result<()> {
        let (node, id) = match &marker.kind {
            RepositoryHistoryDeletionKind::Node(id) => (true, id),
            RepositoryHistoryDeletionKind::User(id) => (false, id),
        };
        self.change_deletion(|state, _| {
            if state.deletion_generation(node, id) != marker.generation {
                return false;
            }
            let mut changed = state.remove_cleanup(destination, id, node);
            changed |= if node {
                state.deleted_nodes.remove(id)
            } else {
                state.deleted_users.remove(id)
            };
            if changed {
                state.retire_generation(node, id);
            }
            changed
        })
        .await
    }
}
