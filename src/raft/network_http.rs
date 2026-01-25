use crate::raft::types::{NodeId, NodeMeta, TypeConfig};

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
    target: NodeId,
    target_node: NodeMeta,
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
            RPCError::Unreachable(openraft::error::Unreachable::new(&e))
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
    ) -> Result<Resp, reqwest::Error> {
        let url = self.url(path);
        tracing::trace!(
            target = "xp::raft::network_http",
            target_id = self.target,
            url = %url,
            timeout_ms = option.hard_ttl().as_millis(),
            "raft rpc send"
        );

        let resp = self
            .client
            .post(url.clone())
            .timeout(option.hard_ttl())
            .json(req)
            .send()
            .await?;
        tracing::trace!(
            target = "xp::raft::network_http",
            target_id = self.target,
            url = %url,
            status = %resp.status(),
            "raft rpc response"
        );
        resp.error_for_status()?.json::<Resp>().await
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
