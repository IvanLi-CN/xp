use std::{collections::BTreeSet, future::Future, pin::Pin, sync::Arc};

use anyhow::Context;
use tokio::sync::watch;

use crate::{
    domain::DomainError,
    raft::types::ClientResponse,
    raft::types::{NodeId, NodeMeta, TypeConfig},
    state::StoreError,
    state::{DesiredStateApplyResult, DesiredStateCommand, JsonSnapshotStore},
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
                (
                    DesiredStateCommand::UpdateGrantFields { grant_id, .. }
                    | DesiredStateCommand::SetGrantEnabled { grant_id, .. },
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
