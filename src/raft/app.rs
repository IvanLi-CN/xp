use std::{collections::BTreeSet, future::Future, pin::Pin, sync::Arc};

use anyhow::Context;
use axum::http::{Method, Uri, header::HeaderName};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::{
    domain::DomainError,
    raft::types::ClientResponse,
    raft::types::{NodeId, NodeMeta, TypeConfig},
    state::StoreError,
    state::{DesiredStateCommand, JsonSnapshotStore},
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

    fn change_membership(
        &self,
        changes: openraft::ChangeMembers<NodeId, NodeMeta>,
        retain: bool,
    ) -> BoxFuture<'_, anyhow::Result<()>>;
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

    fn change_membership(
        &self,
        changes: openraft::ChangeMembers<NodeId, NodeMeta>,
        retain: bool,
    ) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async move {
            self.raft
                .change_membership(changes, retain)
                .await
                .map_err(|e| anyhow::anyhow!("raft change_membership: {e}"))?;
            Ok(())
        })
    }
}

#[derive(Clone)]
pub struct ForwardingRaftFacade {
    raft: openraft::Raft<TypeConfig>,
    metrics: watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>>,
    client: reqwest::Client,
    cluster_ca_key_pem: String,
}

impl ForwardingRaftFacade {
    pub fn try_new(
        raft: openraft::Raft<TypeConfig>,
        cluster_ca_key_pem: String,
        cluster_ca_pem: &str,
        node_cert_pem: Option<&str>,
        node_key_pem: Option<&str>,
    ) -> anyhow::Result<Self> {
        let ca = reqwest::Certificate::from_pem(cluster_ca_pem.as_bytes())
            .context("parse cluster_ca_pem")?;
        let mut builder = reqwest::Client::builder().add_root_certificate(ca);
        if let (Some(cert), Some(key)) = (node_cert_pem, node_key_pem) {
            let identity_pem = format!("{cert}\n{key}");
            let identity = reqwest::Identity::from_pem(identity_pem.as_bytes())
                .context("parse node identity pem")?;
            builder = builder.identity(identity);
        }
        let client = builder.build().context("build reqwest client")?;
        let metrics = raft.metrics();
        Ok(Self {
            raft,
            metrics,
            client,
            cluster_ca_key_pem,
        })
    }
}

impl RaftFacade for ForwardingRaftFacade {
    fn metrics(&self) -> watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>> {
        self.metrics.clone()
    }

    fn client_write(
        &self,
        cmd: DesiredStateCommand,
    ) -> BoxFuture<'_, anyhow::Result<ClientResponse>> {
        let raft = self.raft.clone();
        let metrics = self.metrics.clone();
        let client = self.client.clone();
        let cluster_ca_key_pem = self.cluster_ca_key_pem.clone();
        Box::pin(async move {
            let cmd_clone = cmd.clone();
            match raft.client_write(cmd).await {
                Ok(resp) => Ok(resp.data),
                Err(err) => {
                    let Some(openraft::error::ClientWriteError::ForwardToLeader(forward)) =
                        err.api_error()
                    else {
                        return Err(anyhow::anyhow!("raft client_write: {err}"));
                    };
                    let metrics_snapshot = metrics.borrow().clone();
                    let leader_base_url =
                        leader_api_base_url_from_forward(forward, &metrics_snapshot).ok_or_else(
                            || anyhow::anyhow!("raft client_write forward: leader not available"),
                        )?;
                    forward_client_write(&client, &cluster_ca_key_pem, &leader_base_url, &cmd_clone)
                        .await
                }
            }
        })
    }

    fn add_learner(&self, node_id: NodeId, node: NodeMeta) -> BoxFuture<'_, anyhow::Result<()>> {
        let raft = self.raft.clone();
        Box::pin(async move {
            raft.add_learner(node_id, node, false)
                .await
                .map_err(|e| anyhow::anyhow!("raft add_learner: {e}"))?;
            Ok(())
        })
    }

    fn add_voters(&self, node_ids: BTreeSet<NodeId>) -> BoxFuture<'_, anyhow::Result<()>> {
        let raft = self.raft.clone();
        Box::pin(async move {
            raft.change_membership(openraft::ChangeMembers::AddVoterIds(node_ids), true)
                .await
                .map_err(|e| anyhow::anyhow!("raft change_membership(add_voters): {e}"))?;
            Ok(())
        })
    }

    fn change_membership(
        &self,
        changes: openraft::ChangeMembers<NodeId, NodeMeta>,
        retain: bool,
    ) -> BoxFuture<'_, anyhow::Result<()>> {
        let raft = self.raft.clone();
        let metrics = self.metrics.clone();
        let client = self.client.clone();
        let cluster_ca_key_pem = self.cluster_ca_key_pem.clone();
        Box::pin(async move {
            let changes_clone = changes.clone();
            match raft.change_membership(changes, retain).await {
                Ok(_resp) => Ok(()),
                Err(err) => {
                    let Some(openraft::error::ClientWriteError::ForwardToLeader(forward)) =
                        err.api_error()
                    else {
                        return Err(anyhow::anyhow!("raft change_membership: {err}"));
                    };
                    let metrics_snapshot = metrics.borrow().clone();
                    let leader_base_url = leader_api_base_url_from_forward(
                        forward,
                        &metrics_snapshot,
                    )
                    .ok_or_else(|| {
                        anyhow::anyhow!("raft change_membership forward: leader not available")
                    })?;
                    forward_change_membership(
                        &client,
                        &cluster_ca_key_pem,
                        &leader_base_url,
                        &changes_clone,
                        retain,
                    )
                    .await
                }
            }
        })
    }
}

fn leader_api_base_url_from_forward(
    forward: &openraft::error::ForwardToLeader<NodeId, NodeMeta>,
    metrics: &openraft::RaftMetrics<NodeId, NodeMeta>,
) -> Option<String> {
    if let Some(node) = forward.leader_node.as_ref()
        && !node.api_base_url.is_empty()
    {
        return Some(node.api_base_url.clone());
    }
    let leader_id = forward.leader_id.or(metrics.current_leader)?;
    metrics
        .membership_config
        .nodes()
        .find(|(id, _node)| **id == leader_id)
        .and_then(|(_id, node)| {
            if node.api_base_url.is_empty() {
                None
            } else {
                Some(node.api_base_url.clone())
            }
        })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InternalChangeMembershipRequest {
    retain: bool,
    changes: InternalChangeMembers,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum InternalChangeMembers {
    RemoveVoters { node_ids: Vec<NodeId> },
    RemoveNodes { node_ids: Vec<NodeId> },
}

async fn forward_change_membership(
    client: &reqwest::Client,
    cluster_ca_key_pem: &str,
    leader_base_url: &str,
    changes: &openraft::ChangeMembers<NodeId, NodeMeta>,
    retain: bool,
) -> anyhow::Result<()> {
    let url = format!(
        "{}/api/admin/_internal/raft/change-membership",
        leader_base_url.trim_end_matches('/')
    );
    // Note: the admin auth middleware is attached to the `/admin` nested router, so the
    // verifier sees a stripped path like `/_internal/...` (not `/api/admin/...`).
    let uri: Uri = "/_internal/raft/change-membership"
        .parse()
        .expect("valid uri");
    let sig = crate::internal_auth::sign_request(cluster_ca_key_pem, &Method::POST, &uri)
        .map_err(|e| anyhow::anyhow!("sign internal request: {e}"))?;

    let changes = match changes {
        openraft::ChangeMembers::RemoveVoters(node_ids) => InternalChangeMembers::RemoveVoters {
            node_ids: node_ids.iter().cloned().collect(),
        },
        openraft::ChangeMembers::RemoveNodes(node_ids) => InternalChangeMembers::RemoveNodes {
            node_ids: node_ids.iter().cloned().collect(),
        },
        other => {
            return Err(anyhow::anyhow!(
                "forward change_membership: unsupported change type: {other:?}"
            ));
        }
    };

    client
        .post(url)
        .header(
            HeaderName::from_static(crate::internal_auth::INTERNAL_SIGNATURE_HEADER),
            sig,
        )
        .json(&InternalChangeMembershipRequest { retain, changes })
        .send()
        .await
        .context("forward change_membership request")?
        .error_for_status()
        .context("forward change_membership response status")?;

    Ok(())
}

async fn forward_client_write(
    client: &reqwest::Client,
    cluster_ca_key_pem: &str,
    leader_base_url: &str,
    cmd: &DesiredStateCommand,
) -> anyhow::Result<ClientResponse> {
    let url = format!(
        "{}/api/admin/_internal/raft/client-write",
        leader_base_url.trim_end_matches('/')
    );
    // Note: the admin auth middleware is attached to the `/admin` nested router, so the
    // verifier sees a stripped path like `/_internal/...` (not `/api/admin/...`).
    let uri: Uri = "/_internal/raft/client-write".parse().expect("valid uri");
    let sig = crate::internal_auth::sign_request(cluster_ca_key_pem, &Method::POST, &uri)
        .map_err(|e| anyhow::anyhow!("sign internal request: {e}"))?;
    let resp = client
        .post(url)
        .header(
            HeaderName::from_static(crate::internal_auth::INTERNAL_SIGNATURE_HEADER),
            sig,
        )
        .json(cmd)
        .send()
        .await
        .context("forward client_write request")?
        .error_for_status()
        .context("forward client_write response status")?
        .json::<ClientResponse>()
        .await
        .context("parse forward client_write response")?;
    Ok(resp)
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
            // Local-only cleanup: usage keys for removed memberships should be deleted to keep
            // the local usage file compact (hard-cut behavior).
            let membership_keys_before: Option<std::collections::BTreeSet<String>> = match &cmd {
                DesiredStateCommand::ReplaceUserAccess { user_id, .. }
                | DesiredStateCommand::DeleteUser { user_id } => Some(
                    store
                        .state()
                        .node_user_endpoint_memberships
                        .iter()
                        .filter(|m| m.user_id == *user_id)
                        .map(|m| crate::state::membership_key(&m.user_id, &m.endpoint_id))
                        .collect(),
                ),
                DesiredStateCommand::DeleteEndpoint { endpoint_id } => Some(
                    store
                        .state()
                        .node_user_endpoint_memberships
                        .iter()
                        .filter(|m| m.endpoint_id == *endpoint_id)
                        .map(|m| crate::state::membership_key(&m.user_id, &m.endpoint_id))
                        .collect(),
                ),
                _ => None,
            };
            let out = match cmd.apply(store.state_mut()) {
                Ok(out) => out,
                Err(err) => return Ok(map_store_error(err)),
            };
            store.save().map_err(anyhow::Error::new)?;

            if let Some(before) = membership_keys_before {
                let after: std::collections::BTreeSet<String> = match &cmd {
                    DesiredStateCommand::ReplaceUserAccess { user_id, .. }
                    | DesiredStateCommand::DeleteUser { user_id } => store
                        .state()
                        .node_user_endpoint_memberships
                        .iter()
                        .filter(|m| m.user_id == *user_id)
                        .map(|m| crate::state::membership_key(&m.user_id, &m.endpoint_id))
                        .collect(),
                    DesiredStateCommand::DeleteEndpoint { endpoint_id } => store
                        .state()
                        .node_user_endpoint_memberships
                        .iter()
                        .filter(|m| m.endpoint_id == *endpoint_id)
                        .map(|m| crate::state::membership_key(&m.user_id, &m.endpoint_id))
                        .collect(),
                    _ => std::collections::BTreeSet::new(),
                };
                for membership_key in before.difference(&after) {
                    store
                        .clear_membership_usage(membership_key)
                        .map_err(anyhow::Error::new)?;
                }
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

    fn change_membership(
        &self,
        _changes: openraft::ChangeMembers<NodeId, NodeMeta>,
        _retain: bool,
    ) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async move { Ok(()) })
    }
}

fn map_store_error(err: StoreError) -> ClientResponse {
    match err {
        StoreError::Domain(domain) => match domain {
            DomainError::MissingUser { .. }
            | DomainError::MissingNode { .. }
            | DomainError::MissingEndpoint { .. }
            | DomainError::RealityDomainNotFound { .. } => ClientResponse::Err {
                status: 404,
                code: "not_found".to_string(),
                message: domain.to_string(),
            },
            DomainError::RealityDomainNameConflict { .. } => ClientResponse::Err {
                status: 409,
                code: "conflict".to_string(),
                message: domain.to_string(),
            },
            DomainError::NodeInUse { .. } => ClientResponse::Err {
                status: 409,
                code: "conflict".to_string(),
                message: domain.to_string(),
            },
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
        domain::EndpointKind,
        state::{JsonSnapshotStore, StoreInit, membership_key},
    };

    fn test_store_init(tmp_dir: &Path) -> StoreInit {
        StoreInit {
            data_dir: tmp_dir.to_path_buf(),
            bootstrap_node_id: None,
            bootstrap_node_name: "node-1".to_string(),
            bootstrap_access_host: "".to_string(),
            bootstrap_api_base_url: "https://127.0.0.1:62416".to_string(),
        }
    }

    #[tokio::test]
    async fn replace_user_access_clears_usage_for_removed_memberships_local_raft() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
        let node_id = store.list_nodes()[0].node_id.clone();
        let user = store.create_user("alice".to_string(), None).unwrap();
        let endpoint = store
            .create_endpoint(
                node_id,
                EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8388,
                json!({}),
            )
            .unwrap();
        let membership = membership_key(&user.user_id, &endpoint.endpoint_id);

        DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: vec![endpoint.endpoint_id.clone()],
        }
        .apply(store.state_mut())
        .unwrap();
        store.save().unwrap();

        store
            .set_quota_banned(&membership, "2025-12-18T00:00:00Z".to_string())
            .unwrap();
        assert!(store.get_membership_usage(&membership).is_some());

        let store = Arc::new(Mutex::new(store));
        let (_tx, metrics) = watch::channel(openraft::RaftMetrics::new_initial(0));
        let raft = LocalRaft::new(store.clone(), metrics);

        raft.client_write(DesiredStateCommand::ReplaceUserAccess {
            user_id: user.user_id.clone(),
            endpoint_ids: Vec::new(),
        })
        .await
        .unwrap();

        assert!(
            store
                .lock()
                .await
                .get_membership_usage(&membership)
                .is_none()
        );
    }
}
