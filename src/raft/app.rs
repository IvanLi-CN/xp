use std::{collections::BTreeSet, future::Future, pin::Pin, sync::Arc};

use anyhow::Context;
use tokio::sync::watch;

use crate::{
    domain::DomainError,
    raft::types::ClientResponse,
    raft::types::{NodeId, NodeMeta, TypeConfig},
    state::StoreError,
    state::{DesiredStateApplyResult, DesiredStateCommand, GrantEnabledSource, JsonSnapshotStore},
};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait RaftFacade: Send + Sync + 'static {
    fn metrics(&self) -> watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>>;

    fn client_write(
        &self,
        cmd: DesiredStateCommand,
    ) -> BoxFuture<'_, anyhow::Result<ClientResponse>>;

    fn add_learner(&self, node_id: NodeId, node: NodeMeta) -> BoxFuture<'_, anyhow::Result<()>>;

    fn add_voters(&self, node_ids: BTreeSet<NodeId>) -> BoxFuture<'_, anyhow::Result<()>>;
}

#[derive(Clone)]
pub struct RealRaft {
    raft: openraft::Raft<TypeConfig>,
    metrics: watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>>,
}

impl RealRaft {
    pub fn new(raft: openraft::Raft<TypeConfig>) -> Self {
        let metrics = raft.metrics();
        Self { raft, metrics }
    }

    pub fn raft(&self) -> openraft::Raft<TypeConfig> {
        self.raft.clone()
    }

    pub async fn initialize_single_node_if_needed(
        &self,
        node_id: NodeId,
        node_meta: NodeMeta,
    ) -> anyhow::Result<()> {
        let initialized = self
            .raft
            .is_initialized()
            .await
            .context("raft is_initialized")?;
        if initialized {
            return Ok(());
        }
        let mut nodes = std::collections::BTreeMap::new();
        nodes.insert(node_id, node_meta);
        self.raft
            .initialize(nodes)
            .await
            .map_err(|e| anyhow::anyhow!("raft initialize: {e}"))?;
        Ok(())
    }
}

impl RaftFacade for RealRaft {
    fn metrics(&self) -> watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>> {
        self.metrics.clone()
    }

    fn client_write(
        &self,
        cmd: DesiredStateCommand,
    ) -> BoxFuture<'_, anyhow::Result<ClientResponse>> {
        Box::pin(async move {
            let resp = self
                .raft
                .client_write(cmd)
                .await
                .map_err(|e| anyhow::anyhow!("raft client_write: {e}"))?;
            Ok(resp.data)
        })
    }

    fn add_learner(&self, node_id: NodeId, node: NodeMeta) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async move {
            self.raft
                .add_learner(node_id, node, false)
                .await
                .map_err(|e| anyhow::anyhow!("raft add_learner: {e}"))?;
            Ok(())
        })
    }

    fn add_voters(&self, node_ids: BTreeSet<NodeId>) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async move {
            self.raft
                .change_membership(openraft::ChangeMembers::AddVoterIds(node_ids), true)
                .await
                .map_err(|e| anyhow::anyhow!("raft change_membership(add_voters): {e}"))?;
            Ok(())
        })
    }
}

/// A test-only Raft facade that applies desired-state commands directly to the local store.
#[derive(Clone)]
pub struct LocalRaft {
    store: Arc<tokio::sync::Mutex<JsonSnapshotStore>>,
    metrics: watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>>,
}

impl LocalRaft {
    pub fn new(
        store: Arc<tokio::sync::Mutex<JsonSnapshotStore>>,
        metrics: watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>>,
    ) -> Self {
        Self { store, metrics }
    }
}

impl RaftFacade for LocalRaft {
    fn metrics(&self) -> watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>> {
        self.metrics.clone()
    }

    fn client_write(
        &self,
        cmd: DesiredStateCommand,
    ) -> BoxFuture<'_, anyhow::Result<ClientResponse>> {
        Box::pin(async move {
            let mut store = self.store.lock().await;
            let out = match cmd.apply(store.state_mut()) {
                Ok(out) => out,
                Err(err) => return Ok(map_store_error(err)),
            };
            store.save().map_err(anyhow::Error::new)?;
            // Keep quota/usage non-Raft, but clearing bans on grant edits is a deterministic local side effect.
            match (&cmd, &out) {
                (
                    DesiredStateCommand::DeleteGrant { grant_id },
                    DesiredStateApplyResult::GrantDeleted { deleted },
                ) => {
                    if *deleted {
                        store
                            .clear_grant_usage(grant_id)
                            .map_err(anyhow::Error::new)?;
                    }
                }
                (DesiredStateCommand::UpdateGrantFields { grant_id, .. }, _) => {
                    store
                        .clear_quota_banned(grant_id)
                        .map_err(anyhow::Error::new)?;
                }
                (
                    DesiredStateCommand::SetGrantEnabled {
                        grant_id,
                        source: GrantEnabledSource::Manual,
                        ..
                    },
                    _,
                ) => {
                    store
                        .clear_quota_banned(grant_id)
                        .map_err(anyhow::Error::new)?;
                }
                _ => {}
            }
            Ok(ClientResponse::Ok { result: out })
        })
    }

    fn add_learner(&self, _node_id: NodeId, _node: NodeMeta) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async move { Ok(()) })
    }

    fn add_voters(&self, _node_ids: BTreeSet<NodeId>) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async move { Ok(()) })
    }
}

fn map_store_error(err: StoreError) -> ClientResponse {
    match err {
        StoreError::Domain(domain) => match domain {
            DomainError::MissingUser { .. } | DomainError::MissingEndpoint { .. } => {
                ClientResponse::Err {
                    status: 404,
                    code: "not_found".to_string(),
                    message: domain.to_string(),
                }
            }
            _ => ClientResponse::Err {
                status: 400,
                code: "invalid_request".to_string(),
                message: domain.to_string(),
            },
        },
        other => ClientResponse::Err {
            status: 500,
            code: "internal".to_string(),
            message: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc};

    use serde_json::json;
    use tokio::sync::{Mutex, watch};

    use super::*;
    use crate::{
        domain::{CyclePolicy, CyclePolicyDefault, EndpointKind},
        state::{GrantEnabledSource, JsonSnapshotStore, StoreInit},
    };

    fn test_store_init(tmp_dir: &Path) -> StoreInit {
        StoreInit {
            data_dir: tmp_dir.to_path_buf(),
            bootstrap_node_id: None,
            bootstrap_node_name: "node-1".to_string(),
            bootstrap_public_domain: "".to_string(),
            bootstrap_api_base_url: "https://127.0.0.1:62416".to_string(),
        }
    }

    fn store_with_banned_grant(tmp_dir: &Path) -> (Arc<Mutex<JsonSnapshotStore>>, String) {
        let mut store = JsonSnapshotStore::load_or_init(test_store_init(tmp_dir)).unwrap();
        let node_id = store.list_nodes()[0].node_id.clone();
        let user = store
            .create_user("alice".to_string(), CyclePolicyDefault::ByUser, 1)
            .unwrap();
        let endpoint = store
            .create_endpoint(
                node_id,
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                json!({}),
            )
            .unwrap();
        let grant = store
            .create_grant(
                user.user_id,
                endpoint.endpoint_id,
                1,
                CyclePolicy::InheritUser,
                None,
                None,
            )
            .unwrap();
        store
            .set_quota_banned(&grant.grant_id, "2025-12-18T00:00:00Z".to_string())
            .unwrap();
        (Arc::new(Mutex::new(store)), grant.grant_id)
    }

    #[tokio::test]
    async fn set_grant_enabled_manual_clears_quota_banned_local_raft() {
        let tmp = tempfile::tempdir().unwrap();
        let (store, grant_id) = store_with_banned_grant(tmp.path());
        let (_tx, metrics) = watch::channel(openraft::RaftMetrics::new_initial(0));
        let raft = LocalRaft::new(store.clone(), metrics);

        let cmd = DesiredStateCommand::SetGrantEnabled {
            grant_id: grant_id.clone(),
            enabled: false,
            source: GrantEnabledSource::Manual,
        };
        raft.client_write(cmd).await.unwrap();

        let usage = store.lock().await.get_grant_usage(&grant_id).unwrap();
        assert!(!usage.quota_banned);
    }

    #[tokio::test]
    async fn set_grant_enabled_quota_keeps_quota_banned_local_raft() {
        let tmp = tempfile::tempdir().unwrap();
        let (store, grant_id) = store_with_banned_grant(tmp.path());
        let (_tx, metrics) = watch::channel(openraft::RaftMetrics::new_initial(0));
        let raft = LocalRaft::new(store.clone(), metrics);

        let cmd = DesiredStateCommand::SetGrantEnabled {
            grant_id: grant_id.clone(),
            enabled: false,
            source: GrantEnabledSource::Quota,
        };
        raft.client_write(cmd).await.unwrap();

        let usage = store.lock().await.get_grant_usage(&grant_id).unwrap();
        assert!(usage.quota_banned);
    }
}
