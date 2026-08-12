use std::{path::Path, sync::Arc};

use serde_json::json;
use tokio::sync::{Mutex, watch};

use super::*;
use crate::{
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
