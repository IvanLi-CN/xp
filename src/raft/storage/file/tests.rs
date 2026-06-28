use std::{path::Path, sync::Arc};

use serde_json::json;
use tokio::sync::{Mutex, mpsc};

use super::*;
use crate::{
    domain::{
        Endpoint, EndpointKind, Node, NodeQuotaReset, QuotaResetSource, User, UserQuotaReset,
    },
    reconcile::ReconcileRequest,
    state::{JsonSnapshotStore, StoreInit, UserNodeQuotaConfig},
};

fn test_store_init(tmp_dir: &Path) -> StoreInit {
    StoreInit {
        data_dir: tmp_dir.to_path_buf(),
        bootstrap_node_id: None,
        bootstrap_node_name: "node-1".to_string(),
        bootstrap_access_host: "".to_string(),
        bootstrap_api_base_url: "https://127.0.0.1:62416".to_string(),
    }
}

fn build_entry(cmd: DesiredStateCommand, index: u64) -> openraft::impls::Entry<TypeConfig> {
    let log_id = LogId::new(openraft::CommittedLeaderId::new(1, 1), index);
    openraft::impls::Entry {
        log_id,
        payload: EntryPayload::Normal(cmd),
    }
}

fn build_retired_grant_group_raw_entry(index: u64, cmd_type: &str) -> serde_json::Value {
    let log_id = LogId::new(openraft::CommittedLeaderId::new(1, 1), index);
    let mut raw_entry = serde_json::to_value(openraft::impls::Entry::<TypeConfig> {
        log_id,
        payload: EntryPayload::Blank,
    })
    .unwrap();
    raw_entry["payload"] = json!({
        "normal": {
            "type": cmd_type,
        }
    });
    raw_entry
}

#[tokio::test]
async fn upsert_endpoint_change_requests_rebuild_inbound() {
    let tmp = tempfile::tempdir().unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let reconcile = ReconcileHandle::from_sender(tx);
    let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
    let store = Arc::new(Mutex::new(store));
    let endpoint_id = {
        let mut store = store.lock().await;
        let node_id = store.list_nodes()[0].node_id.clone();
        let endpoint = store
            .create_endpoint(
                node_id,
                EndpointKind::VlessRealityVisionTcp,
                443,
                json!({
                    "reality": {
                        "dest": "example.com:443",
                        "server_names": ["example.com"],
                        "fingerprint": "chrome"
                    }
                }),
            )
            .unwrap();
        endpoint.endpoint_id
    };

    let mut state_machine = FileStateMachine::open(tmp.path(), store.clone(), reconcile)
        .await
        .unwrap();

    let mut endpoint = {
        let store = store.lock().await;
        store.get_endpoint(&endpoint_id).unwrap()
    };
    endpoint.port = 8443;

    let entry = build_entry(DesiredStateCommand::UpsertEndpoint { endpoint }, 1);
    state_machine.apply(vec![entry]).await.unwrap();

    let mut requests = Vec::new();
    while let Ok(req) = rx.try_recv() {
        requests.push(req);
    }
    assert!(requests.iter().any(|req| {
        matches!(
            req,
            ReconcileRequest::RebuildInbound { endpoint_id: id } if id == &endpoint_id
        )
    }));
}

#[tokio::test]
async fn install_snapshot_migrates_legacy_grants_state_to_v10() {
    let tmp = tempfile::tempdir().unwrap();
    let reconcile = ReconcileHandle::noop();
    let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
    let store = Arc::new(Mutex::new(store));
    let mut state_machine = FileStateMachine::open(tmp.path(), store.clone(), reconcile)
        .await
        .unwrap();

    let node = Node {
        node_id: "node_1".to_string(),
        node_name: "node-1".to_string(),
        access_host: "example.com".to_string(),
        api_base_url: "https://127.0.0.1:62416".to_string(),
        quota_limit_bytes: 0,
        quota_reset: NodeQuotaReset::default(),
    };
    let endpoint = Endpoint {
        endpoint_id: "endpoint_1".to_string(),
        node_id: node.node_id.clone(),
        tag: "ss".to_string(),
        kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
        port: 8388,
        meta: json!({}),
    };
    let user = User {
        user_id: "user_1".to_string(),
        display_name: "alice".to_string(),
        subscription_token: "sub_1".to_string(),
        credential_epoch: 0,
        priority_tier: Default::default(),
        quota_reset: UserQuotaReset::default(),
    };

    let legacy_snapshot = json!({
        "state": {
            "schema_version": 9,
            "nodes": {
                node.node_id.clone(): node,
            },
            "endpoints": {
                endpoint.endpoint_id.clone(): endpoint,
            },
            "users": {
                user.user_id.clone(): user,
            },
            "grants": {
                "grant_1": {
                    "grant_id": "grant_1",
                    "user_id": "user_1",
                    "endpoint_id": "endpoint_1",
                    "enabled": true,
                },
            },
            "user_node_quotas": {
                "user_1": {
                    "node_1": UserNodeQuotaConfig {
                        quota_limit_bytes: Some(100 * 1024 * 1024 * 1024),
                        quota_reset_source: QuotaResetSource::User,
                    }
                }
            }
        }
    });

    let bytes = serde_json::to_vec_pretty(&legacy_snapshot).unwrap();
    let meta = SnapshotMeta {
        last_log_id: None,
        last_membership: StoredMembership::default(),
        snapshot_id: "snapshot-test".to_string(),
    };

    state_machine
        .install_snapshot(&meta, Box::new(std::io::Cursor::new(bytes)))
        .await
        .unwrap();

    let store = store.lock().await;
    assert_eq!(store.state().schema_version, crate::state::SCHEMA_VERSION);
    assert!(store.state().user_node_quotas.is_empty());
    assert!(
        store
            .state()
            .node_user_endpoint_memberships
            .iter()
            .any(|m| m.user_id == "user_1"
                && m.endpoint_id == "endpoint_1"
                && m.node_id == "node_1")
    );
}

#[tokio::test]
async fn read_wal_with_compat_rejects_non_array_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let wal_path = tmp.path().join("wal.json");
    let raw = serde_json::to_vec(&json!({
        "last_purged_log_id": null,
        "entries": {
            "unexpected": true
        }
    }))
    .unwrap();
    std::fs::write(&wal_path, raw).unwrap();

    let err = read_wal_with_compat(&wal_path, Some(0)).await.unwrap_err();
    assert!(err.to_string().contains("entries"));
    assert!(err.to_string().contains("array"));
}

#[tokio::test]
async fn read_wal_with_compat_rejects_unapplied_retired_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let wal_path = tmp.path().join("wal.json");
    let raw_entry = build_retired_grant_group_raw_entry(5, "create_grant_group");
    let last_purged_log_id =
        serde_json::to_value(LogId::new(openraft::CommittedLeaderId::new(1, 1), 5)).unwrap();
    let raw = serde_json::to_vec(&json!({
        "last_purged_log_id": last_purged_log_id,
        "entries": [raw_entry]
    }))
    .unwrap();
    std::fs::write(&wal_path, raw).unwrap();

    let err = read_wal_with_compat(&wal_path, Some(4)).await.unwrap_err();
    assert!(err.to_string().contains("not applied yet"));
}

#[tokio::test]
async fn read_wal_with_compat_rejects_unpurged_retired_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let wal_path = tmp.path().join("wal.json");
    let raw_entry = build_retired_grant_group_raw_entry(5, "replace_grant_group");
    let last_purged_log_id =
        serde_json::to_value(LogId::new(openraft::CommittedLeaderId::new(1, 1), 4)).unwrap();
    let raw = serde_json::to_vec(&json!({
        "last_purged_log_id": last_purged_log_id,
        "entries": [raw_entry]
    }))
    .unwrap();
    std::fs::write(&wal_path, raw).unwrap();

    let err = read_wal_with_compat(&wal_path, Some(10)).await.unwrap_err();
    assert!(err.to_string().contains("active log range"));
}

#[tokio::test]
async fn read_wal_with_compat_rewrites_purged_retired_entry_to_blank() {
    let tmp = tempfile::tempdir().unwrap();
    let wal_path = tmp.path().join("wal.json");
    let raw_entry = build_retired_grant_group_raw_entry(5, "delete_grant_group");
    let last_purged_log_id =
        serde_json::to_value(LogId::new(openraft::CommittedLeaderId::new(1, 1), 5)).unwrap();
    let raw = serde_json::to_vec(&json!({
        "last_purged_log_id": last_purged_log_id,
        "entries": [raw_entry]
    }))
    .unwrap();
    std::fs::write(&wal_path, raw).unwrap();

    let (wal, rewritten) = read_wal_with_compat(&wal_path, Some(10)).await.unwrap();
    assert!(rewritten);
    assert_eq!(wal.entries.len(), 1);
    assert!(matches!(wal.entries[0].payload, EntryPayload::Blank));
}
