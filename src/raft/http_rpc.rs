use std::sync::Arc;

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderName, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::post,
};
use tokio::sync::Mutex;

use crate::{
    domain::Node,
    internal_auth::{self, InternalRoute},
    raft::types::{NodeId, TypeConfig},
    state::JsonSnapshotStore,
};

use openraft::error::RaftError;

#[derive(Clone)]
pub struct RaftRpcState {
    pub raft: openraft::Raft<TypeConfig>,
}

/// Authentication context used by the production Raft RPC router. Keeping it separate from the
/// Raft state preserves the small in-process router used by replication tests.
#[derive(Clone)]
pub struct RaftRpcAuth {
    pub cluster_id: String,
    pub local_node_id: String,
    pub cluster_ca_key_pem: String,
    pub cluster_ca_cert_pem: String,
    pub store: Arc<Mutex<JsonSnapshotStore>>,
}

pub fn build_raft_rpc_router(state: RaftRpcState) -> Router {
    build_raft_rpc_router_inner(state, None)
}

pub fn build_authenticated_raft_rpc_router(state: RaftRpcState, auth: RaftRpcAuth) -> Router {
    build_raft_rpc_router_inner(state, Some(auth))
}

fn build_raft_rpc_router_inner(state: RaftRpcState, auth: Option<RaftRpcAuth>) -> Router {
    let router = Router::new()
        .route("/raft/append", post(append_entries))
        .route("/raft/vote", post(vote))
        .route("/raft/snapshot", post(install_snapshot));
    let router = match auth {
        Some(auth) => router.layer(middleware::from_fn_with_state(auth, raft_auth)),
        None => router,
    };
    router.with_state(state)
}

async fn raft_auth(State(auth): State<RaftRpcAuth>, req: Request<Body>, next: Next) -> Response {
    const RAFT_BODY_LIMIT: usize = 8 * 1024 * 1024;

    let (parts, body) = req.into_parts();
    let bytes = match to_bytes(body, RAFT_BODY_LIMIT).await {
        Ok(bytes) => bytes,
        Err(_) => return (StatusCode::PAYLOAD_TOO_LARGE, "raft body is too large").into_response(),
    };
    let req = Request::from_parts(parts, Body::from(bytes.clone()));
    let verified = match internal_auth::verify_request_v2(
        &auth.cluster_ca_key_pem,
        &auth.cluster_ca_cert_pem,
        req.method(),
        req.uri(),
        req.headers(),
        &bytes,
        &auth.cluster_id,
        &auth.local_node_id,
    ) {
        Ok(verified) if verified.context.route == InternalRoute::MeshV2 => verified,
        Ok(verified) => {
            tracing::warn!(
                sender_id = %verified.context.sender_id,
                route = %verified.context.route.as_str(),
                "rejected Raft request with an invalid internal route"
            );
            return (StatusCode::UNAUTHORIZED, "invalid raft authentication").into_response();
        }
        Err(error) => {
            tracing::warn!(error = %error, "rejected unauthenticated Raft request");
            return (StatusCode::UNAUTHORIZED, "invalid raft authentication").into_response();
        }
    };
    let store = auth.store.lock().await;
    let sender_is_member = store.get_node(&verified.context.sender_id).is_some();
    let nodes = store.list_nodes();
    // A freshly joined node has no replicated state yet. Its first authenticated Raft request
    // is what installs the member list, so requiring the sender to already exist would make
    // bootstrap impossible. The CA signature still authenticates the request; once any state
    // exists, keep the membership check strict so removed nodes cannot resume replication.
    let initial_bootstrap = is_initial_raft_bootstrap(&nodes, &auth.local_node_id);
    drop(store);
    if !sender_is_member && !initial_bootstrap {
        tracing::warn!(
            sender_id = %verified.context.sender_id,
            "rejected Raft request from a non-member"
        );
        return (
            StatusCode::UNAUTHORIZED,
            "raft sender is not a cluster member",
        )
            .into_response();
    }

    let mut response = next.run(req).await;
    if let Ok(ack) = internal_auth::sign_ack_v2(
        &auth.cluster_ca_key_pem,
        &auth.cluster_ca_cert_pem,
        &verified,
        &auth.local_node_id,
        response.status().as_u16(),
    ) && let Ok(value) = ack.parse()
    {
        response.headers_mut().insert(
            HeaderName::from_static(internal_auth::INTERNAL_ACK_HEADER),
            value,
        );
    }
    response
}

fn is_initial_raft_bootstrap(nodes: &[Node], local_node_id: &str) -> bool {
    nodes.is_empty() || (nodes.len() == 1 && nodes[0].node_id == local_node_id)
}

// The handlers deserialize only after `raft_auth` verifies a signed body, membership, and target.

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::NodeQuotaReset;

    fn primary_node() -> Node {
        Node {
            node_id: xp_test_fixtures::primary_node_id().to_owned(),
            node_name: xp_test_fixtures::primary_node_name().to_owned(),
            access_host: xp_test_fixtures::primary_host().to_owned(),
            api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        }
    }

    fn secondary_node() -> Node {
        Node {
            node_id: xp_test_fixtures::secondary_node_id().to_owned(),
            node_name: xp_test_fixtures::secondary_node_name().to_owned(),
            access_host: xp_test_fixtures::secondary_host().to_owned(),
            api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        }
    }

    #[test]
    fn initial_raft_bootstrap_only_allows_empty_or_self_only_state() {
        let self_id = xp_test_fixtures::primary_node_id();
        assert!(is_initial_raft_bootstrap(&[], self_id));
        assert!(is_initial_raft_bootstrap(&[primary_node()], self_id));
        assert!(!is_initial_raft_bootstrap(&[secondary_node()], self_id));
        assert!(!is_initial_raft_bootstrap(
            &[primary_node(), secondary_node()],
            self_id
        ));
    }
}
