use std::sync::Arc;
use std::{collections::BTreeSet, path::PathBuf};

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderName, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::post,
};
use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

use crate::{
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
    pub bootstrap_sender: Option<BootstrapSenderMarker>,
}

#[derive(Clone)]
pub struct BootstrapSenderMarker {
    pub sender_ids: BTreeSet<String>,
    pub activation_deadline: DateTime<Utc>,
    pub reverse_mesh: Option<crate::reverse_mesh::ReverseMeshBootstrapMarker>,
    pub path: PathBuf,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct BootstrapSenderMarkerFile {
    sender_ids: BTreeSet<String>,
    activation_deadline: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reverse_mesh: Option<crate::reverse_mesh::ReverseMeshBootstrapMarker>,
}

pub fn encode_bootstrap_sender_marker(
    sender_ids: &[String],
    activation_deadline: &str,
) -> Result<Vec<u8>, serde_json::Error> {
    encode_bootstrap_sender_marker_with_reverse(sender_ids, activation_deadline, None)
}

pub fn encode_bootstrap_sender_marker_with_reverse(
    sender_ids: &[String],
    activation_deadline: &str,
    reverse_mesh: Option<&crate::reverse_mesh::ReverseMeshBootstrapMarker>,
) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&BootstrapSenderMarkerFile {
        sender_ids: sender_ids.iter().cloned().collect(),
        activation_deadline: activation_deadline.to_string(),
        reverse_mesh: reverse_mesh.cloned(),
    })
}

pub fn read_bootstrap_sender_marker(path: PathBuf) -> Option<BootstrapSenderMarker> {
    let raw = std::fs::read_to_string(&path).ok()?;
    let file = serde_json::from_str::<BootstrapSenderMarkerFile>(&raw).ok()?;
    let activation_deadline = DateTime::parse_from_rfc3339(&file.activation_deadline)
        .ok()?
        .with_timezone(&Utc);
    if activation_deadline <= Utc::now() || file.sender_ids.is_empty() {
        let _ = std::fs::remove_file(path);
        return None;
    }
    Some(BootstrapSenderMarker {
        sender_ids: file.sender_ids,
        activation_deadline,
        reverse_mesh: file.reverse_mesh,
        path,
    })
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
    let sender_is_member = {
        let store = auth.store.lock().await;
        store.get_node(&verified.context.sender_id).is_some()
    };
    let marker_sender_ids = active_bootstrap_sender_ids(auth.bootstrap_sender.as_ref());
    let bootstrap_sender = bootstrap_sender_is_allowed(
        &verified.context.sender_id,
        sender_is_member,
        marker_sender_ids,
    );
    if !sender_is_member && !bootstrap_sender {
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
    if response.status().is_success()
        && let Some(marker) = auth.bootstrap_sender.as_ref()
    {
        let replicated_node_ids = auth
            .store
            .lock()
            .await
            .list_nodes()
            .into_iter()
            .map(|node| node.node_id)
            .collect::<BTreeSet<_>>();
        if bootstrap_replication_complete(Some(&marker.sender_ids), &replicated_node_ids) {
            let _ = std::fs::remove_file(&marker.path);
        }
    }
    response
}

fn bootstrap_sender_is_allowed(
    sender_id: &str,
    sender_is_member: bool,
    marker_sender_ids: Option<&BTreeSet<String>>,
) -> bool {
    !sender_is_member && marker_sender_ids.is_some_and(|markers| markers.contains(sender_id))
}

fn active_bootstrap_sender_ids(
    bootstrap_sender: Option<&BootstrapSenderMarker>,
) -> Option<&BTreeSet<String>> {
    bootstrap_sender.and_then(|marker| {
        (marker.activation_deadline > Utc::now() && std::fs::metadata(&marker.path).is_ok())
            .then_some(&marker.sender_ids)
    })
}

fn bootstrap_replication_complete(
    marker_sender_ids: Option<&BTreeSet<String>>,
    replicated_node_ids: &BTreeSet<String>,
) -> bool {
    marker_sender_ids.is_some_and(|markers| {
        markers
            .iter()
            .all(|sender_id| replicated_node_ids.contains(sender_id))
    })
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
    use std::collections::BTreeSet;

    use super::{
        BootstrapSenderMarker, active_bootstrap_sender_ids, bootstrap_replication_complete,
        bootstrap_sender_is_allowed, encode_bootstrap_sender_marker_with_reverse,
        read_bootstrap_sender_marker,
    };

    #[test]
    fn bootstrap_sender_requires_marker_and_membership_bypass() {
        assert!(bootstrap_sender_is_allowed(
            "leader",
            false,
            Some(&BTreeSet::from(["leader".to_string()]))
        ));
        assert!(!bootstrap_sender_is_allowed(
            "leader",
            true,
            Some(&BTreeSet::from(["leader".to_string()]))
        ));
        assert!(!bootstrap_sender_is_allowed("leader", false, None));
    }

    #[test]
    fn bootstrap_sender_rejects_a_different_authenticated_principal() {
        assert!(!bootstrap_sender_is_allowed(
            "removed-node",
            false,
            Some(&BTreeSet::from(["elected-leader".to_string()]))
        ));
    }

    #[test]
    fn bootstrap_sender_allows_a_current_voter_after_failover() {
        let voters = BTreeSet::from(["old-leader".to_string(), "new-leader".to_string()]);
        assert!(bootstrap_sender_is_allowed(
            "new-leader",
            false,
            Some(&voters)
        ));
    }

    #[test]
    fn bootstrap_marker_waits_for_all_recorded_voters() {
        let voters = BTreeSet::from(["old-leader".to_string(), "new-leader".to_string()]);
        let only_old = BTreeSet::from(["old-leader".to_string()]);
        assert!(!bootstrap_replication_complete(Some(&voters), &only_old));
        assert!(bootstrap_replication_complete(Some(&voters), &voters));
    }

    #[test]
    fn bootstrap_marker_cache_stops_authorizing_after_file_removal() {
        let temp = tempfile::tempdir().expect("marker directory");
        let marker_path = temp.path().join("raft_bootstrap_sender");
        std::fs::write(&marker_path, "marker").expect("write marker");
        let marker = BootstrapSenderMarker {
            sender_ids: BTreeSet::from(["leader".to_string()]),
            activation_deadline: chrono::Utc::now() + chrono::Duration::minutes(1),
            reverse_mesh: None,
            path: marker_path.clone(),
        };
        assert!(active_bootstrap_sender_ids(Some(&marker)).is_some());
        std::fs::remove_file(marker_path).expect("remove marker");
        assert!(active_bootstrap_sender_ids(Some(&marker)).is_none());
    }

    #[test]
    fn bootstrap_marker_round_trips_public_reverse_parameters() {
        let temp = tempfile::tempdir().expect("marker directory");
        let marker_path = temp.path().join("raft_bootstrap_sender");
        let reverse = crate::reverse_mesh::ReverseMeshBootstrapMarker {
            epoch: 4,
            generation: 2,
            target_node_id: "target".to_string(),
            primary_node_id: "primary".to_string(),
            standby_node_id: Some("standby".to_string()),
            primary_endpoint: crate::reverse_mesh::ReverseMeshBootstrapEndpoint {
                access_host: xp_test_fixtures::primary_host().to_owned(),
                port: 443,
                server_name: xp_test_fixtures::primary_server_name().to_owned(),
                public_key: "public".to_string(),
                short_id: "0123456789abcdef".to_string(),
                transport: "vision_tcp".to_string(),
            },
            standby_endpoint: None,
        };
        let bytes = encode_bootstrap_sender_marker_with_reverse(
            &["primary".to_string()],
            "2099-01-01T00:00:00Z",
            Some(&reverse),
        )
        .expect("encode marker");
        std::fs::write(&marker_path, bytes).expect("write marker");
        let parsed = read_bootstrap_sender_marker(marker_path).expect("read marker");
        assert_eq!(parsed.reverse_mesh, Some(reverse));
    }

    #[test]
    fn expired_bootstrap_marker_is_removed_and_not_authorized() {
        let temp = tempfile::tempdir().expect("marker directory");
        let marker_path = temp.path().join("raft_bootstrap_sender");
        let marker = serde_json::json!({
            "sender_ids": ["leader"],
            "activation_deadline": "2020-01-01T00:00:00Z"
        });
        std::fs::write(&marker_path, marker.to_string()).expect("write marker");
        assert!(super::read_bootstrap_sender_marker(marker_path.clone()).is_none());
        assert!(!marker_path.exists());
    }
}
