use std::{path::Path, sync::Arc};

use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::Response,
    routing::any,
};
use serde_json::json;
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
};

use super::*;
use crate::{
    control_plane_mesh::MeshRequestError,
    domain::{Endpoint, EndpointKind},
    state::{JsonSnapshotStore, StoreInit, membership_key},
};

fn test_store_init(tmp_dir: &Path) -> StoreInit {
    StoreInit {
        data_dir: tmp_dir.to_path_buf(),
        bootstrap_node_id: None,
        bootstrap_node_name: xp_test_fixtures::label_node1_variant2().to_owned(),
        bootstrap_access_host: xp_test_fixtures::label_empty().to_owned(),
        bootstrap_api_base_url: xp_test_fixtures::subscription_api_loopback_https().to_owned(),
    }
}

#[derive(Clone)]
struct ForwardResponseState {
    ca_key_pem: String,
    ca_cert_pem: String,
    cluster_id: String,
    sender_id: String,
    target_id: String,
    status: StatusCode,
    body: String,
}

async fn forward_response(
    State(state): State<ForwardResponseState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let verified = crate::internal_auth::verify_request_v2(
        &state.ca_key_pem,
        &state.ca_cert_pem,
        &method,
        &uri,
        &headers,
        &body,
        &state.cluster_id,
        &state.target_id,
    )
    .expect("forwarded write request is signed");
    assert_eq!(verified.context.sender_id, state.sender_id);
    let acknowledgement = crate::internal_auth::sign_ack_v2(
        &state.ca_key_pem,
        &state.ca_cert_pem,
        &verified,
        &state.target_id,
        state.status.as_u16(),
    )
    .expect("forwarded write acknowledgement");
    Response::builder()
        .status(state.status)
        .header(crate::internal_auth::INTERNAL_ACK_HEADER, acknowledgement)
        .body(axum::body::Body::from(state.body))
        .expect("forwarded write response")
}

async fn spawn_forward_response(state: ForwardResponseState) -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("forward response listener");
    let address = listener.local_addr().expect("forward response address");
    let task = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            Router::new()
                .fallback(any(forward_response))
                .with_state(state),
        )
        .await;
    });
    (format!("http://{address}"), task)
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

#[tokio::test]
async fn conditional_endpoint_upsert_returns_conflict_for_stale_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
    let node_id = store.list_nodes()[0].node_id.clone();
    let endpoint = store
        .create_endpoint(
            node_id,
            EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            8388,
            json!({}),
        )
        .unwrap();
    let expected = endpoint.clone();
    let mut updated = endpoint;
    updated.port = 8443;
    DesiredStateCommand::UpsertEndpoint {
        endpoint: updated,
        expected: None,
    }
    .apply(store.state_mut())
    .unwrap();

    let store = Arc::new(Mutex::new(store));
    let (_tx, metrics) = watch::channel(openraft::RaftMetrics::new_initial(0));
    let raft = LocalRaft::new(store, metrics);
    let response = raft
        .client_write(DesiredStateCommand::UpsertEndpoint {
            endpoint: Endpoint {
                port: 9443,
                ..expected.clone()
            },
            expected: Some(expected),
        })
        .await
        .unwrap();

    assert!(matches!(
        response,
        ClientResponse::Err { status: 409, ref code, .. } if code == "conflict"
    ));
}

#[test]
fn conditional_endpoint_update_requires_the_new_capability() {
    let endpoint = Endpoint {
        endpoint_id: xp_test_fixtures::endpoint_id_fixture538().to_owned(),
        node_id: xp_test_fixtures::identifier_ulid_d().to_owned(),
        tag: xp_test_fixtures::endpoint_tag_fixture539().to_owned(),
        kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
        port: 8388,
        meta: serde_json::json!({}),
    };

    assert!(command_requires_conditional_endpoint_update(
        &DesiredStateCommand::UpsertEndpoint {
            endpoint: endpoint.clone(),
            expected: Some(endpoint.clone()),
        }
    ));
    assert!(!command_requires_conditional_endpoint_update(
        &DesiredStateCommand::UpsertEndpoint {
            endpoint,
            expected: None,
        }
    ));
    assert!(capabilities_support_conditional_endpoint_update(&[
        "admin.endpoints".to_string(),
        CONDITIONAL_ENDPOINT_UPDATE_CAPABILITY.to_string(),
    ]));
    assert!(!capabilities_support_conditional_endpoint_update(&[
        "admin.endpoints".to_string(),
    ]));
}

#[test]
fn raft_forwarding_error_classification_is_conservative() {
    assert_eq!(
        crate::raft::write_error::dispatch_state(&MeshRequestError::InvalidTarget(
            "bad".to_string(),
        )),
        RaftWriteDispatchState::NotDispatched
    );
    assert_eq!(
        crate::raft::write_error::dispatch_state(&MeshRequestError::PreDispatchTimeout),
        RaftWriteDispatchState::NotDispatched
    );
    assert_eq!(
        crate::raft::write_error::dispatch_state(&MeshRequestError::TransportTimeout),
        RaftWriteDispatchState::DispatchedNoVerifiedResponse
    );
    assert_eq!(
        crate::raft::write_error::dispatch_state(&MeshRequestError::OutcomeUnknown),
        RaftWriteDispatchState::Unknown
    );
    assert_eq!(
        crate::raft::write_error::dispatch_state(&MeshRequestError::CircuitOpen {
            path: "Public",
            dispatched: true,
        }),
        RaftWriteDispatchState::Unknown
    );
}

#[tokio::test]
async fn forwarded_client_write_failures_keep_safe_outcome_details() {
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let target_id = xp_test_fixtures::primary_node_id().to_owned();
    let sender_id = xp_test_fixtures::tertiary_node_id().to_owned();
    let cluster_id = xp_test_fixtures::cluster_fixture53().to_owned();
    let command = DesiredStateCommand::SetMeshEnabled { enabled: false };
    let node = crate::domain::Node {
        node_id: target_id.clone(),
        node_name: xp_test_fixtures::primary_node_name().to_owned(),
        access_host: xp_test_fixtures::primary_host().to_owned(),
        api_base_url: String::new(),
        quota_limit_bytes: 0,
        quota_reset: Default::default(),
    };

    for (status, body, cause, remote_status) in [
        (
            StatusCode::BAD_GATEWAY,
            r#"{}"#,
            "remote_error_response",
            Some(502),
        ),
        (StatusCode::OK, "not-json", "response_decode", None),
    ] {
        let (public_base_url, task) = spawn_forward_response(ForwardResponseState {
            ca_key_pem: ca.key_pem.clone(),
            ca_cert_pem: ca.cert_pem.clone(),
            cluster_id: cluster_id.clone(),
            sender_id: sender_id.clone(),
            target_id: target_id.clone(),
            status,
            body: body.to_owned(),
        })
        .await;
        let mut peer = peer_target_from_node(&node, &[]);
        peer.public_base_url = public_base_url;
        let error = forward_client_write(
            &MeshAwareHttpClient::new(reqwest::Client::new()),
            &ForwardingAuth {
                cluster_ca_key_pem: &ca.key_pem,
                cluster_ca_pem: &ca.cert_pem,
                cluster_id: &cluster_id,
                sender_id: &sender_id,
            },
            &peer,
            &command,
            "support-forwarded-write",
        )
        .await
        .expect_err("forwarded write failure should be typed");
        let typed = error
            .downcast_ref::<RaftWriteError>()
            .expect("forwarded write error should remain typed");
        assert_eq!(typed.cause(), cause);
        assert_eq!(
            typed.dispatch_state(),
            RaftWriteDispatchState::DispatchedNoVerifiedResponse
        );
        assert_eq!(typed.remote_status(), remote_status);
        task.abort();
    }
}
