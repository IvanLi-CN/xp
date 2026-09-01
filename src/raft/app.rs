use std::{any::Any, collections::BTreeSet, future::Future, pin::Pin, sync::Arc, time::Duration};

use anyhow::Context;
use futures_util::FutureExt;
use serde::Deserialize;
use tokio::sync::watch;

use crate::{
    control_plane_mesh::{MeshAwareHttpClient, MeshPeerTarget, MeshRequest, peer_target_from_node},
    domain::DomainError,
    internal_auth::InternalRoute,
    raft::types::ClientResponse,
    raft::types::{NodeId, NodeMeta, TypeConfig},
    state::StoreError,
    state::{DesiredStateCommand, JsonSnapshotStore},
};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

const CONDITIONAL_ENDPOINT_UPDATE_CAPABILITY: &str = "admin.endpoint-conditional-update";

fn panic_payload_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(msg) = payload.downcast_ref::<&str>() {
        return (*msg).to_string();
    }
    if let Some(msg) = payload.downcast_ref::<String>() {
        return msg.clone();
    }
    "unknown panic payload".to_string()
}

async fn catch_raft_panic<T, F>(label: &'static str, fut: F) -> anyhow::Result<T>
where
    F: Future<Output = T> + Send,
{
    std::panic::AssertUnwindSafe(fut)
        .catch_unwind()
        .await
        .map_err(|payload| anyhow::anyhow!("{label} panicked: {}", panic_payload_message(payload)))
}

pub trait RaftFacade: Send + Sync + 'static {
    fn metrics(&self) -> watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>>;

    /// Confirm that the local node still owns a quorum-backed, linearizable view before a
    /// membership precondition is evaluated. Test facades use the conservative metrics check;
    /// production facades override this with OpenRaft's quorum heartbeat protocol.
    fn ensure_linearizable(&self) -> BoxFuture<'_, anyhow::Result<()>> {
        let metrics = self.metrics();
        Box::pin(async move {
            let snapshot = metrics.borrow().clone();
            if snapshot.state != openraft::ServerState::Leader
                || snapshot.current_leader != Some(snapshot.id)
            {
                anyhow::bail!(
                    "linearizable membership check requires local leader: \
                     state={:?}, current_leader={:?}, local={}",
                    snapshot.state,
                    snapshot.current_leader,
                    snapshot.id
                );
            }
            Ok(())
        })
    }

    fn client_write(
        &self,
        cmd: DesiredStateCommand,
    ) -> BoxFuture<'_, anyhow::Result<ClientResponse>>;

    fn add_learner(&self, node_id: NodeId, node: NodeMeta) -> BoxFuture<'_, anyhow::Result<()>>;

    fn wait_learner_caught_up(
        &self,
        node_id: NodeId,
        required_log_index: u64,
        timeout: Duration,
    ) -> BoxFuture<'_, anyhow::Result<()>> {
        let mut metrics = self.metrics();
        Box::pin(async move {
            let expected_leader = {
                let snapshot = metrics.borrow().clone();
                if snapshot.state != openraft::ServerState::Leader {
                    anyhow::bail!("not leader while waiting for learner catch-up");
                }
                snapshot.id
            };
            let deadline = tokio::time::Instant::now() + timeout;
            loop {
                {
                    let snapshot = metrics.borrow();
                    if snapshot.state != openraft::ServerState::Leader
                        || snapshot.current_leader != Some(expected_leader)
                    {
                        anyhow::bail!("leadership changed while waiting for learner catch-up");
                    }

                    let membership = snapshot.membership_config.membership();
                    if membership.voter_ids().any(|voter_id| voter_id == node_id) {
                        return Ok(());
                    }
                    if membership.get_node(&node_id).is_some()
                        && let Some(replication) = snapshot.replication.as_ref()
                        && let Some(Some(log_id)) = replication.get(&node_id)
                        && log_id.index >= required_log_index
                    {
                        return Ok(());
                    }
                }

                let now = tokio::time::Instant::now();
                if now >= deadline {
                    anyhow::bail!("timeout after {}s", timeout.as_secs());
                }
                tokio::time::timeout(deadline - now, metrics.changed())
                    .await
                    .map_err(|_| anyhow::anyhow!("timeout after {}s", timeout.as_secs()))?
                    .map_err(|e| anyhow::anyhow!("raft metrics channel closed: {e}"))?;
            }
        })
    }

    fn add_voters(&self, node_ids: BTreeSet<NodeId>) -> BoxFuture<'_, anyhow::Result<()>>;

    fn change_membership(
        &self,
        changes: openraft::ChangeMembers<NodeId, NodeMeta>,
        retain: bool,
    ) -> BoxFuture<'_, anyhow::Result<()>>;
}

#[derive(Clone)]
pub struct PanicBoundaryRaft {
    inner: Arc<dyn RaftFacade>,
}

impl PanicBoundaryRaft {
    pub fn wrap(inner: Arc<dyn RaftFacade>) -> Arc<dyn RaftFacade> {
        Arc::new(Self { inner })
    }
}

impl RaftFacade for PanicBoundaryRaft {
    fn metrics(&self) -> watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>> {
        self.inner.metrics()
    }

    fn ensure_linearizable(&self) -> BoxFuture<'_, anyhow::Result<()>> {
        let inner = self.inner.clone();
        Box::pin(async move {
            catch_raft_panic("raft ensure_linearizable", inner.ensure_linearizable()).await??;
            Ok(())
        })
    }

    fn client_write(
        &self,
        cmd: DesiredStateCommand,
    ) -> BoxFuture<'_, anyhow::Result<ClientResponse>> {
        let inner = self.inner.clone();
        Box::pin(
            async move { catch_raft_panic("raft client_write", inner.client_write(cmd)).await? },
        )
    }

    fn add_learner(&self, node_id: NodeId, node: NodeMeta) -> BoxFuture<'_, anyhow::Result<()>> {
        let inner = self.inner.clone();
        Box::pin(async move {
            catch_raft_panic("raft add_learner", inner.add_learner(node_id, node)).await??;
            Ok(())
        })
    }

    fn wait_learner_caught_up(
        &self,
        node_id: NodeId,
        required_log_index: u64,
        timeout: Duration,
    ) -> BoxFuture<'_, anyhow::Result<()>> {
        let inner = self.inner.clone();
        Box::pin(async move {
            catch_raft_panic(
                "raft wait_learner_caught_up",
                inner.wait_learner_caught_up(node_id, required_log_index, timeout),
            )
            .await??;
            Ok(())
        })
    }

    fn add_voters(&self, node_ids: BTreeSet<NodeId>) -> BoxFuture<'_, anyhow::Result<()>> {
        let inner = self.inner.clone();
        Box::pin(async move {
            catch_raft_panic(
                "raft change_membership(add_voters)",
                inner.add_voters(node_ids),
            )
            .await??;
            Ok(())
        })
    }

    fn change_membership(
        &self,
        changes: openraft::ChangeMembers<NodeId, NodeMeta>,
        retain: bool,
    ) -> BoxFuture<'_, anyhow::Result<()>> {
        let inner = self.inner.clone();
        Box::pin(async move {
            catch_raft_panic(
                "raft change_membership",
                inner.change_membership(changes, retain),
            )
            .await??;
            Ok(())
        })
    }
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

    fn ensure_linearizable(&self) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async move {
            catch_raft_panic("raft ensure_linearizable", self.raft.ensure_linearizable())
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .map_err(|e| anyhow::anyhow!("raft ensure_linearizable: {e}"))?;
            Ok(())
        })
    }

    fn client_write(
        &self,
        cmd: DesiredStateCommand,
    ) -> BoxFuture<'_, anyhow::Result<ClientResponse>> {
        Box::pin(async move {
            let resp = catch_raft_panic("raft client_write", self.raft.client_write(cmd))
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .map_err(|e| anyhow::anyhow!("raft client_write: {e}"))?;
            Ok(resp.data)
        })
    }

    fn add_learner(&self, node_id: NodeId, node: NodeMeta) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async move {
            catch_raft_panic(
                "raft add_learner",
                self.raft.add_learner(node_id, node, false),
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .map_err(|e| anyhow::anyhow!("raft add_learner: {e}"))?;
            Ok(())
        })
    }

    fn add_voters(&self, node_ids: BTreeSet<NodeId>) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async move {
            catch_raft_panic(
                "raft change_membership(add_voters)",
                self.raft
                    .change_membership(openraft::ChangeMembers::AddVoterIds(node_ids), true),
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?
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
            catch_raft_panic(
                "raft change_membership",
                self.raft.change_membership(changes, retain),
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .map_err(|e| anyhow::anyhow!("raft change_membership: {e}"))?;
            Ok(())
        })
    }
}

#[derive(Clone)]
pub struct ForwardingRaftFacade {
    raft: openraft::Raft<TypeConfig>,
    metrics: watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>>,
    mesh_client: MeshAwareHttpClient,
    cluster_ca_key_pem: String,
    cluster_ca_pem: String,
    cluster_id: String,
    local_node_id: String,
    store: Arc<tokio::sync::Mutex<JsonSnapshotStore>>,
}

impl ForwardingRaftFacade {
    pub fn try_new(
        raft: openraft::Raft<TypeConfig>,
        mesh_client: MeshAwareHttpClient,
        cluster_ca_key_pem: String,
        cluster_ca_pem: &str,
        cluster_id: String,
        local_node_id: String,
        store: Arc<tokio::sync::Mutex<JsonSnapshotStore>>,
    ) -> anyhow::Result<Self> {
        let metrics = raft.metrics();
        Ok(Self {
            raft,
            metrics,
            mesh_client,
            cluster_ca_key_pem,
            cluster_ca_pem: cluster_ca_pem.to_string(),
            cluster_id,
            local_node_id,
            store,
        })
    }
}

impl RaftFacade for ForwardingRaftFacade {
    fn metrics(&self) -> watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>> {
        self.metrics.clone()
    }

    fn ensure_linearizable(&self) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async move {
            catch_raft_panic("raft ensure_linearizable", self.raft.ensure_linearizable())
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .map_err(|e| anyhow::anyhow!("raft ensure_linearizable: {e}"))?;
            Ok(())
        })
    }

    fn client_write(
        &self,
        cmd: DesiredStateCommand,
    ) -> BoxFuture<'_, anyhow::Result<ClientResponse>> {
        let raft = self.raft.clone();
        let metrics = self.metrics.clone();
        let mesh_client = self.mesh_client.clone();
        let cluster_ca_key_pem = self.cluster_ca_key_pem.clone();
        let cluster_ca_pem = self.cluster_ca_pem.clone();
        let cluster_id = self.cluster_id.clone();
        let local_node_id = self.local_node_id.clone();
        let store = self.store.clone();
        Box::pin(async move {
            let cmd_clone = cmd.clone();
            match catch_raft_panic("raft client_write", raft.client_write(cmd)).await? {
                Ok(resp) => Ok(resp.data),
                Err(err) => {
                    if lifecycle_command_requires_local_leader(&cmd_clone) {
                        return Err(anyhow::anyhow!(concat!(
                            "membership lifecycle leader changed; retry the recorded operation ",
                            "on the current leader"
                        )));
                    }
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
                    let target_id =
                        leader_target_id(&store, forward, &metrics_snapshot, &leader_base_url)
                            .await?;
                    let peer = forwarding_peer_target(&store, &target_id, &leader_base_url).await?;
                    forward_client_write(
                        &mesh_client,
                        &ForwardingAuth {
                            cluster_ca_key_pem: &cluster_ca_key_pem,
                            cluster_ca_pem: &cluster_ca_pem,
                            cluster_id: &cluster_id,
                            sender_id: &local_node_id,
                        },
                        &peer,
                        &cmd_clone,
                    )
                    .await
                }
            }
        })
    }

    fn add_learner(&self, node_id: NodeId, node: NodeMeta) -> BoxFuture<'_, anyhow::Result<()>> {
        let raft = self.raft.clone();
        Box::pin(async move {
            catch_raft_panic("raft add_learner", raft.add_learner(node_id, node, false))
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .map_err(|e| anyhow::anyhow!("raft add_learner: {e}"))?;
            Ok(())
        })
    }

    fn add_voters(&self, node_ids: BTreeSet<NodeId>) -> BoxFuture<'_, anyhow::Result<()>> {
        let raft = self.raft.clone();
        Box::pin(async move {
            catch_raft_panic(
                "raft change_membership(add_voters)",
                raft.change_membership(openraft::ChangeMembers::AddVoterIds(node_ids), true),
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?
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
        Box::pin(async move {
            catch_raft_panic(
                "raft change_membership",
                raft.change_membership(changes, retain),
            )
            .await?
            .map_err(|err| anyhow::anyhow!("raft change_membership: {err}"))?;
            Ok(())
        })
    }
}

fn lifecycle_command_requires_local_leader(cmd: &DesiredStateCommand) -> bool {
    matches!(
        cmd,
        DesiredStateCommand::UpsertNode { .. }
            | DesiredStateCommand::DeleteNode { .. }
            | DesiredStateCommand::BeginMembershipOperation { .. }
            | DesiredStateCommand::TransitionMembershipOperation { .. }
            | DesiredStateCommand::PruneMembershipOperations { .. }
    )
}

async fn leader_target_id(
    store: &Arc<tokio::sync::Mutex<JsonSnapshotStore>>,
    forward: &openraft::error::ForwardToLeader<NodeId, NodeMeta>,
    metrics: &openraft::RaftMetrics<NodeId, NodeMeta>,
    leader_base_url: &str,
) -> anyhow::Result<String> {
    let leader_id = forward
        .leader_id
        .or(metrics.current_leader)
        .ok_or_else(|| anyhow::anyhow!("raft forward: leader id is unavailable"))?;
    let store = store.lock().await;
    store
        .list_nodes()
        .into_iter()
        .find(|node| {
            crate::raft::types::raft_node_id_from_ulid(&node.node_id)
                .is_ok_and(|node_id| node_id == leader_id)
                || node.api_base_url.trim_end_matches('/') == leader_base_url.trim_end_matches('/')
        })
        .map(|node| node.node_id)
        .ok_or_else(|| anyhow::anyhow!("raft forward: leader is not a current cluster member"))
}

async fn forwarding_peer_target(
    store: &Arc<tokio::sync::Mutex<JsonSnapshotStore>>,
    target_id: &str,
    leader_base_url: &str,
) -> anyhow::Result<MeshPeerTarget> {
    let store = store.lock().await;
    let node = store
        .get_node(target_id)
        .or_else(|| {
            store.list_nodes().into_iter().find(|node| {
                node.api_base_url.trim_end_matches('/') == leader_base_url.trim_end_matches('/')
            })
        })
        .ok_or_else(|| anyhow::anyhow!("raft forward: leader is not a current cluster member"))?;
    Ok(peer_target_from_node(&node, &store.list_endpoints()))
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

struct ForwardingAuth<'a> {
    cluster_ca_key_pem: &'a str,
    cluster_ca_pem: &'a str,
    cluster_id: &'a str,
    sender_id: &'a str,
}

async fn send_forwarded_mesh_request(
    client: &MeshAwareHttpClient,
    auth: &ForwardingAuth<'_>,
    peer: &MeshPeerTarget,
    path_and_query: &str,
    body: Vec<u8>,
    allow_ambiguous_fallback: bool,
) -> anyhow::Result<reqwest::Response> {
    client
        .send_peer_request(
            peer,
            MeshRequest {
                method: reqwest::Method::POST,
                path_and_query: path_and_query.to_string(),
                content_type: Some("application/json".to_string()),
                body,
                total_budget: Duration::from_secs(10),
                allow_ambiguous_fallback,
                request_id: crate::id::new_ulid_string(),
                route: InternalRoute::MeshV2,
                cluster_id: auth.cluster_id.to_string(),
                sender_id: auth.sender_id.to_string(),
                updates_active_path: true,
            },
            auth.cluster_ca_key_pem,
            auth.cluster_ca_pem,
        )
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))
}

async fn forward_client_write(
    client: &MeshAwareHttpClient,
    auth: &ForwardingAuth<'_>,
    peer: &MeshPeerTarget,
    cmd: &DesiredStateCommand,
) -> anyhow::Result<ClientResponse> {
    if command_requires_conditional_endpoint_update(cmd)
        && !leader_supports_conditional_endpoint_update(client, peer)
            .await
            .unwrap_or(false)
    {
        return Ok(ClientResponse::Err {
            status: 409,
            code: "coordinated_upgrade_required".to_string(),
            message: "endpoint PATCH requires a leader that supports conditional endpoint updates"
                .to_string(),
        });
    }
    let body = serde_json::to_vec(cmd)?;
    let response = send_forwarded_mesh_request(
        client,
        auth,
        peer,
        "/api/admin/_internal/raft/client-write",
        body,
        true,
    )
    .await
    .context("forward client_write request")?;
    if !response.status().is_success() {
        anyhow::bail!(
            "forward client_write response status: {}",
            response.status()
        );
    }
    let response = response
        .json::<ClientResponse>()
        .await
        .context("parse forward client_write response")?;
    Ok(response)
}

fn command_requires_conditional_endpoint_update(cmd: &DesiredStateCommand) -> bool {
    matches!(
        cmd,
        DesiredStateCommand::UpsertEndpoint {
            expected: Some(_),
            ..
        }
    )
}

#[derive(Deserialize)]
struct CapabilitiesResponse {
    #[serde(default)]
    capabilities: Vec<String>,
}

async fn leader_supports_conditional_endpoint_update(
    client: &MeshAwareHttpClient,
    peer: &MeshPeerTarget,
) -> anyhow::Result<bool> {
    let url = format!(
        "{}/api/capabilities",
        peer.public_base_url.trim_end_matches('/')
    );
    let response = tokio::time::timeout(Duration::from_secs(5), client.direct().get(url).send())
        .await
        .map_err(|_| anyhow::anyhow!("leader capability request timed out"))??;
    if !response.status().is_success() {
        return Ok(false);
    }
    let capabilities = response
        .json::<CapabilitiesResponse>()
        .await
        .context("parse leader capabilities")?;
    Ok(capabilities_support_conditional_endpoint_update(
        &capabilities.capabilities,
    ))
}

fn capabilities_support_conditional_endpoint_update(capabilities: &[String]) -> bool {
    capabilities
        .iter()
        .any(|capability| capability == CONDITIONAL_ENDPOINT_UPDATE_CAPABILITY)
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
            // Local-only cleanup: usage keys and inbound IP history for removed memberships
            // should be deleted to keep local files compact (hard-cut behavior).
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
                    store
                        .clear_membership_inbound_ip_usage(membership_key)
                        .map_err(anyhow::Error::new)?;
                }
            }
            match &cmd {
                DesiredStateCommand::DeleteEndpoint { endpoint_id } => {
                    store
                        .clear_endpoint_tcp_connection_usage(endpoint_id)
                        .map_err(anyhow::Error::new)?;
                }
                DesiredStateCommand::DeleteNode { node_id, .. } => {
                    store
                        .clear_node_tcp_connection_usage(node_id)
                        .map_err(anyhow::Error::new)?;
                }
                DesiredStateCommand::UpsertEndpoint { .. }
                | DesiredStateCommand::UpsertNode { .. } => {
                    store
                        .prune_tcp_connection_usage_endpoints()
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

    fn wait_learner_caught_up(
        &self,
        _node_id: NodeId,
        _required_log_index: u64,
        _timeout: Duration,
    ) -> BoxFuture<'_, anyhow::Result<()>> {
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
            | DomainError::MissingServiceMonitor { .. }
            | DomainError::RealityDomainNotFound { .. } => ClientResponse::Err {
                status: 404,
                code: "not_found".to_string(),
                message: domain.to_string(),
            },
            DomainError::NodeInUse { .. }
            | DomainError::NodeEndpointSetChanged { .. }
            | DomainError::NodeLifecycleOperationActive { .. }
            | DomainError::EndpointChanged { .. }
            | DomainError::ServiceMonitorExists { .. }
            | DomainError::ServiceMonitorChanged { .. } => ClientResponse::Err {
                status: 409,
                code: "conflict".to_string(),
                message: domain.to_string(),
            },
            DomainError::RealityDomainNameConflict { .. } => ClientResponse::Err {
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
#[path = "app/tests.rs"]
mod tests;
