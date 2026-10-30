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
    pub(crate) async fn lock_user_deletion(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.user_deletion_lock.lock().await
    }
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

    /// Persist the intended fanout before the Raft deletion; preparation alone never deletes data.
    pub(crate) async fn prepare_user_deletion(
        &self,
        user_id: &str,
        destinations: &[String],
    ) -> anyhow::Result<()> {
        self.change_deletion(|state, _| {
            let was_present = state.prepared_user_deletions.contains_key(user_id);
            let pending = state
                .prepared_user_deletions
                .entry(user_id.to_owned())
                .or_default();
            let before = pending.len();
            pending.extend(destinations.iter().cloned());
            !was_present || before != pending.len()
        })
        .await
    }

    /// Activate one prepared operation only after committed Raft state proves the user absent.
    pub(crate) async fn activate_prepared_user_deletion(
        &self,
        user_id: &str,
        current_destinations: &[String],
    ) -> anyhow::Result<()> {
        self.change_deletion(|state, generation| {
            let Some(mut destinations) = state.prepared_user_deletions.remove(user_id) else {
                return false;
            };
            destinations.extend(current_destinations.iter().cloned());
            state.deleted_users.insert(user_id.to_owned());
            state
                .deletion_generations
                .insert(generation_key(false, user_id), generation);
            for record in state.nodes.values_mut() {
                record.user_traffic.remove(user_id);
                record.user_traffic_users.remove(user_id);
            }
            for destination in destinations {
                state
                    .pending_user_traffic_cleanup
                    .entry(destination)
                    .or_default()
                    .insert(user_id.to_owned());
            }
            true
        })
        .await
    }

    /// Resume prepared deletions even when the original request stopped before its Raft write.
    pub(crate) async fn recover_prepared_user_deletions(
        &self,
        store: &Arc<Mutex<JsonSnapshotStore>>,
        raft: &dyn crate::raft::app::RaftFacade,
        local_node_id: &str,
    ) -> anyhow::Result<usize> {
        const MAX_RECOVERY_BATCH: usize = 16;
        let users = {
            let state = self.inner.read().await;
            state
                .prepared_user_deletions
                .keys()
                .take(MAX_RECOVERY_BATCH)
                .cloned()
                .collect::<Vec<_>>()
        };
        let mut completed = 0;
        for user in &users {
            let _guard = self.lock_user_deletion().await;
            if !self
                .inner
                .read()
                .await
                .prepared_user_deletions
                .contains_key(user)
            {
                continue;
            }
            let present = store.lock().await.get_user(user).is_some();
            if present {
                let response = tokio::time::timeout(
                    Duration::from_secs(3),
                    raft.client_write(crate::state::DesiredStateCommand::DeleteUser {
                        user_id: user.clone(),
                    }),
                )
                .await;
                if !matches!(
                    response,
                    Ok(Ok(crate::raft::types::ClientResponse::Ok {
                        result: crate::state::DesiredStateApplyResult::UserDeleted { .. }
                    }))
                ) {
                    warn!(
                        user_id = %user,
                        "prepared user deletion is waiting for Raft; retry retained"
                    );
                    continue;
                }
            }
            let store = store.lock().await;
            let destinations = store
                .list_nodes()
                .into_iter()
                .filter(|node| node.node_id != local_node_id)
                .map(|node| node.node_id)
                .collect::<Vec<_>>();
            self.activate_prepared_user_deletion(user, &destinations)
                .await?;
            completed += 1;
        }
        Ok(completed)
    }

    /// Node membership removal is already Raft-durable; install the whole fanout atomically.
    pub(crate) async fn clear_node_with_destinations(
        &self,
        node_id: &str,
        destinations: &[String],
    ) -> anyhow::Result<()> {
        self.change_deletion(|state, generation| {
            state.deleted_nodes.insert(node_id.to_owned());
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
            for destination in destinations {
                state
                    .pending_node_history_cleanup
                    .entry(destination.clone())
                    .or_default()
                    .insert(node_id.to_owned());
            }
            state.prune_generations();
            true
        })
        .await
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
