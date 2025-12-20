use axum::{Json, Router, extract::State, routing::post};

use crate::raft::types::{NodeId, TypeConfig};

use openraft::error::RaftError;

#[derive(Clone)]
pub struct RaftRpcState {
    pub raft: openraft::Raft<TypeConfig>,
}

pub fn build_raft_rpc_router(state: RaftRpcState) -> Router {
    Router::new()
        .route("/raft/append", post(append_entries))
        .route("/raft/vote", post(vote))
        .route("/raft/snapshot", post(install_snapshot))
        .with_state(state)
}

async fn append_entries(
    State(state): State<RaftRpcState>,
    Json(req): Json<openraft::raft::AppendEntriesRequest<TypeConfig>>,
) -> Json<Result<openraft::raft::AppendEntriesResponse<NodeId>, RaftError<NodeId>>> {
    Json(state.raft.append_entries(req).await)
}

async fn vote(
    State(state): State<RaftRpcState>,
    Json(req): Json<openraft::raft::VoteRequest<NodeId>>,
) -> Json<Result<openraft::raft::VoteResponse<NodeId>, RaftError<NodeId>>> {
    Json(state.raft.vote(req).await)
}

async fn install_snapshot(
    State(state): State<RaftRpcState>,
    Json(req): Json<openraft::raft::InstallSnapshotRequest<TypeConfig>>,
) -> Json<
    Result<
        openraft::raft::InstallSnapshotResponse<NodeId>,
        RaftError<NodeId, openraft::error::InstallSnapshotError>,
    >,
> {
    Json(state.raft.install_snapshot(req).await)
}
