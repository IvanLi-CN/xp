use crate::raft::types::{NodeId, NodeMeta, TypeConfig};

use anyhow::Context;
use openraft::{
    RaftNetwork, RaftNetworkFactory,
    error::{RPCError, RaftError},
    network::RPCOption,
    raft::{
        AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest,
        InstallSnapshotResponse, VoteRequest, VoteResponse,
    },
};

#[derive(Clone)]
pub struct HttpNetworkFactory {
    client: reqwest::Client,
}

impl HttpNetworkFactory {
    pub fn new() -> Self {
        let client = reqwest::Client::builder().build().expect("reqwest client");
        Self { client }
    }

    pub fn from_client(client: reqwest::Client) -> Self {
        Self { client }
    }

    pub fn try_new_mtls(
        cluster_ca_pem: &str,
        node_cert_pem: &str,
        node_key_pem: &str,
    ) -> anyhow::Result<Self> {
        let ca = reqwest::Certificate::from_pem(cluster_ca_pem.as_bytes())
            .context("parse cluster_ca_pem")?;
        let identity_pem = format!("{node_cert_pem}\n{node_key_pem}");
        let identity = reqwest::Identity::from_pem(identity_pem.as_bytes())
            .context("parse node identity pem")?;

        let client = reqwest::Client::builder()
            .tls_built_in_root_certs(false)
            .add_root_certificate(ca)
            .identity(identity)
            .build()
            .context("build reqwest client")?;
        Ok(Self { client })
    }
}

impl Default for HttpNetworkFactory {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct HttpNetwork {
    base: String,
    client: reqwest::Client,
}

impl HttpNetwork {
    fn url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    async fn post_json<Req: serde::Serialize, Resp: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        req: &Req,
        option: RPCOption,
    ) -> Result<Resp, reqwest::Error> {
        self.client
            .post(self.url(path))
            .timeout(option.hard_ttl())
            .json(req)
            .send()
            .await?
            .json::<Resp>()
            .await
    }
}

impl RaftNetworkFactory<TypeConfig> for HttpNetworkFactory {
    type Network = HttpNetwork;

    async fn new_client(&mut self, target: NodeId, node: &NodeMeta) -> Self::Network {
        let _ = target;
        HttpNetwork {
            base: node.raft_endpoint.clone(),
            client: self.client.clone(),
        }
    }
}

impl RaftNetwork<TypeConfig> for HttpNetwork {
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<TypeConfig>,
        option: RPCOption,
    ) -> Result<AppendEntriesResponse<NodeId>, RPCError<NodeId, NodeMeta, RaftError<NodeId>>> {
        let res: Result<
            AppendEntriesResponse<NodeId>,
            RPCError<NodeId, NodeMeta, RaftError<NodeId>>,
        > = self
            .post_json("/raft/append", &rpc, option)
            .await
            .map_err(|e| RPCError::Unreachable(openraft::error::Unreachable::new(&e)))?;
        res
    }

    async fn install_snapshot(
        &mut self,
        rpc: InstallSnapshotRequest<TypeConfig>,
        option: RPCOption,
    ) -> Result<
        InstallSnapshotResponse<NodeId>,
        RPCError<NodeId, NodeMeta, RaftError<NodeId, openraft::error::InstallSnapshotError>>,
    > {
        let res: Result<
            InstallSnapshotResponse<NodeId>,
            RPCError<NodeId, NodeMeta, RaftError<NodeId, openraft::error::InstallSnapshotError>>,
        > = self
            .post_json("/raft/snapshot", &rpc, option)
            .await
            .map_err(|e| RPCError::Unreachable(openraft::error::Unreachable::new(&e)))?;
        res
    }

    async fn vote(
        &mut self,
        rpc: VoteRequest<NodeId>,
        option: RPCOption,
    ) -> Result<VoteResponse<NodeId>, RPCError<NodeId, NodeMeta, RaftError<NodeId>>> {
        let res: Result<VoteResponse<NodeId>, RPCError<NodeId, NodeMeta, RaftError<NodeId>>> = self
            .post_json("/raft/vote", &rpc, option)
            .await
            .map_err(|e| RPCError::Unreachable(openraft::error::Unreachable::new(&e)))?;
        res
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
