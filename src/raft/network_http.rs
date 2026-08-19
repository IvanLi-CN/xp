use crate::{
    control_plane_mesh::{
        MeshAwareHttpClient, MeshPeerTarget, MeshRequest, ReverseRelayRoute,
        build_mesh_http_client, build_unauthenticated_mesh_http_client, peer_target_from_node,
    },
    internal_auth::InternalRoute,
    mesh_telemetry::MeshTelemetryHandle,
    raft::types::{NodeId, NodeMeta, TypeConfig},
    state::JsonSnapshotStore,
};

use anyhow::Context;
use openraft::{
    RaftNetwork, RaftNetworkFactory,
    error::{RPCError, RaftError, RemoteError},
    network::RPCOption,
    raft::{
        AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest,
        InstallSnapshotResponse, VoteRequest, VoteResponse,
    },
};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct RaftMeshAuth {
    pub cluster_id: String,
    pub sender_id: String,
    pub cluster_ca_key_pem: String,
    pub cluster_ca_cert_pem: String,
    pub store: Arc<Mutex<JsonSnapshotStore>>,
}

#[derive(Clone)]
pub struct HttpNetworkFactory {
    client: MeshAwareHttpClient,
    mesh_auth: Option<RaftMeshAuth>,
}

impl HttpNetworkFactory {
    pub fn new() -> Self {
        Self {
            client: build_unauthenticated_mesh_http_client()
                .expect("build unauthenticated Mesh transport clients"),
            mesh_auth: None,
        }
    }

    pub fn from_client(client: reqwest::Client) -> Self {
        Self {
            client: MeshAwareHttpClient::new(client),
            mesh_auth: None,
        }
    }

    pub fn try_new_mtls(
        cluster_ca_pem: &str,
        node_cert_pem: &str,
        node_key_pem: &str,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            client: build_mesh_http_client(cluster_ca_pem, node_cert_pem, node_key_pem)
                .context("build Mesh transport clients")?,
            mesh_auth: None,
        })
    }

    pub fn try_new_mtls_with_mesh_auth(
        cluster_ca_pem: &str,
        node_cert_pem: &str,
        node_key_pem: &str,
        mesh_auth: RaftMeshAuth,
    ) -> anyhow::Result<Self> {
        let mut factory = Self::try_new_mtls(cluster_ca_pem, node_cert_pem, node_key_pem)?;
        factory.mesh_auth = Some(mesh_auth);
        Ok(factory)
    }

    pub fn with_mesh_observability(mut self, telemetry: MeshTelemetryHandle) -> Self {
        self.client = self.client.with_mesh_observability(telemetry);
        self
    }

    pub fn with_local_reverse_relay(
        mut self,
        node_id: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        self.client = self.client.with_local_reverse_relay(node_id, base_url);
        self
    }

    pub fn with_reverse_gate(mut self, gate: Arc<AtomicBool>) -> Self {
        self.client = self.client.with_reverse_gate(gate);
        self
    }

    pub fn mesh_client(&self) -> MeshAwareHttpClient {
        self.client.clone()
    }
}

impl Default for HttpNetworkFactory {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct HttpNetwork {
    target: NodeId,
    target_node: NodeMeta,
    base: String,
    client: MeshAwareHttpClient,
    mesh_auth: Option<RaftMeshAuth>,
}

impl HttpNetwork {
    fn url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    async fn post_raft_result<
        Req: serde::Serialize,
        Resp: serde::de::DeserializeOwned,
        Err: std::error::Error + serde::de::DeserializeOwned,
    >(
        &self,
        path: &str,
        req: &Req,
        option: RPCOption,
    ) -> Result<Resp, RPCError<NodeId, NodeMeta, Err>> {
        let result: Result<Resp, Err> = self.post_json(path, req, option).await.map_err(|e| {
            tracing::warn!(
                target = "xp::raft::network_http",
                target_id = self.target,
                url = %self.url(path),
                error = %e,
                "raft rpc unreachable"
            );
            RPCError::Unreachable(openraft::error::Unreachable::new(&std::io::Error::other(
                e.to_string(),
            )))
        })?;

        match result {
            Ok(resp) => Ok(resp),
            Err(err) => Err(RPCError::RemoteError(RemoteError::new_with_node(
                self.target,
                self.target_node.clone(),
                err,
            ))),
        }
    }

    async fn post_json<Req: serde::Serialize, Resp: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        req: &Req,
        option: RPCOption,
    ) -> anyhow::Result<Resp> {
        let url = self.url(path);
        tracing::trace!(
            target = "xp::raft::network_http",
            target_id = self.target,
            url = %url,
            timeout_ms = option.hard_ttl().as_millis(),
            "raft rpc send"
        );

        let resp = if let Some(mesh_auth) = &self.mesh_auth {
            let body = serde_json::to_vec(req).context("serialize raft rpc")?;
            let target =
                mesh_target_for_raft(&mesh_auth.store, &self.base, &self.target_node).await;
            configure_reverse_route_for_raft(&mesh_auth.store, &self.client, &target).await;
            self.client
                .send_peer_request(
                    &target,
                    MeshRequest {
                        method: reqwest::Method::POST,
                        path_and_query: path.to_string(),
                        content_type: Some("application/json".to_string()),
                        body,
                        total_budget: option.hard_ttl(),
                        // Raft RPCs are protocol-idempotent. The same request id is preserved
                        // across the Mesh/public attempt inside this call.
                        allow_ambiguous_fallback: true,
                        request_id: ulid::Ulid::new().to_string(),
                        route: InternalRoute::MeshV2,
                        cluster_id: mesh_auth.cluster_id.clone(),
                        sender_id: mesh_auth.sender_id.clone(),
                        updates_active_path: true,
                    },
                    &mesh_auth.cluster_ca_key_pem,
                    &mesh_auth.cluster_ca_cert_pem,
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?
        } else {
            self.client
                .direct()
                .post(url.clone())
                .timeout(option.hard_ttl())
                .json(req)
                .send()
                .await?
        };
        tracing::trace!(
            target = "xp::raft::network_http",
            target_id = self.target,
            url = %url,
            status = %resp.status(),
            "raft rpc response"
        );
        Ok(resp.error_for_status()?.json::<Resp>().await?)
    }
}

async fn mesh_target_for_raft(
    store: &Arc<Mutex<JsonSnapshotStore>>,
    raft_base_url: &str,
    target_node: &NodeMeta,
) -> MeshPeerTarget {
    let store = store.lock().await;
    let peer = store.list_nodes().into_iter().find(|node| {
        node.api_base_url == target_node.api_base_url || node.api_base_url == raft_base_url
    });
    let Some(peer) = peer else {
        return MeshPeerTarget {
            node_id: target_node.name.clone(),
            node_name: target_node.name.clone(),
            mesh_base_url: None,
            mesh_reason: crate::mesh_telemetry::MeshPeerReason::MissingEndpoint,
            public_base_url: raft_base_url.to_string(),
        };
    };
    peer_target_from_node(&peer, &store.list_endpoints())
}

async fn configure_reverse_route_for_raft(
    store: &Arc<Mutex<JsonSnapshotStore>>,
    client: &MeshAwareHttpClient,
    target: &MeshPeerTarget,
) {
    let route = {
        let store = store.lock().await;
        (|| {
            let assignment = store
                .state()
                .reverse_mesh_assignments
                .get(&target.node_id)
                .cloned()?;
            let rendezvous = store.get_node(&assignment.primary_node_id)?;
            let standby_rendezvous = assignment
                .standby_node_id
                .as_deref()
                .and_then(|standby_id| store.get_node(standby_id))
                .map(|standby| {
                    let endpoints = store
                        .list_endpoints()
                        .into_iter()
                        .filter(|endpoint| endpoint.node_id == standby.node_id)
                        .collect::<Vec<_>>();
                    crate::control_plane_mesh::peer_target_from_node(&standby, &endpoints)
                });
            let endpoints = store
                .list_endpoints()
                .into_iter()
                .filter(|endpoint| endpoint.node_id == rendezvous.node_id)
                .collect::<Vec<_>>();
            Some(ReverseRelayRoute {
                rendezvous: peer_target_from_node(&rendezvous, &endpoints),
                standby_rendezvous,
                assignment,
                role: crate::reverse_mesh::ReverseRole::Primary,
            })
        })()
    };
    match route {
        Some(route) => {
            client
                .set_reverse_route(target.node_id.clone(), route)
                .await
        }
        None => client.clear_reverse_route(&target.node_id).await,
    }
}

impl RaftNetworkFactory<TypeConfig> for HttpNetworkFactory {
    type Network = HttpNetwork;

    async fn new_client(&mut self, target: NodeId, node: &NodeMeta) -> Self::Network {
        HttpNetwork {
            target,
            target_node: node.clone(),
            base: node.raft_endpoint.clone(),
            client: self.client.clone(),
            mesh_auth: self.mesh_auth.clone(),
        }
    }
}

impl RaftNetwork<TypeConfig> for HttpNetwork {
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<TypeConfig>,
        option: RPCOption,
    ) -> Result<AppendEntriesResponse<NodeId>, RPCError<NodeId, NodeMeta, RaftError<NodeId>>> {
        self.post_raft_result("/raft/append", &rpc, option).await
    }

    async fn install_snapshot(
        &mut self,
        rpc: InstallSnapshotRequest<TypeConfig>,
        option: RPCOption,
    ) -> Result<
        InstallSnapshotResponse<NodeId>,
        RPCError<NodeId, NodeMeta, RaftError<NodeId, openraft::error::InstallSnapshotError>>,
    > {
        self.post_raft_result("/raft/snapshot", &rpc, option).await
    }

    async fn vote(
        &mut self,
        rpc: VoteRequest<NodeId>,
        option: RPCOption,
    ) -> Result<VoteResponse<NodeId>, RPCError<NodeId, NodeMeta, RaftError<NodeId>>> {
        self.post_raft_result("/raft/vote", &rpc, option).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_mtls_network_factory_succeeds() {
        let cluster_id = "01JTESTCLUSTERID00000000000000";
        let node_id = "01JTESTNODEID0000000000000000";

        let ca = crate::cluster_identity::generate_cluster_ca(cluster_id).expect("cluster ca");
        let csr =
            crate::cluster_identity::generate_node_keypair_and_csr(node_id).expect("node csr");
        let cert = crate::cluster_identity::sign_node_csr(cluster_id, &ca.key_pem, &csr.csr_pem)
            .expect("sign node csr");

        let _factory =
            HttpNetworkFactory::try_new_mtls(&ca.cert_pem, &cert, &csr.key_pem).expect("mtls");
    }
}

#[cfg(test)]
mod transport_reuse_tests;
