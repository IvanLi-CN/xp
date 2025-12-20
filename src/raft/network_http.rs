use crate::raft::types::{NodeId, NodeMeta, TypeConfig};

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
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .expect("reqwest client");
        Self { client }
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
