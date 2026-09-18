use std::{path::Path, sync::Arc};

use serde_json::json;
use tokio::sync::Mutex;

use super::*;
use crate::state::StoreInit;

fn test_store_init(tmp_dir: &Path) -> StoreInit {
    StoreInit {
        data_dir: tmp_dir.to_path_buf(),
        bootstrap_node_id: None,
        bootstrap_node_name: xp_test_fixtures::label_node1_variant2().to_owned(),
        bootstrap_access_host: "".to_string(),
        bootstrap_api_base_url: xp_test_fixtures::subscription_api_loopback_https().to_owned(),
    }
}

fn closed(reconcile: &ReconcileHandle) -> bool {
    !reconcile
        .mesh_gate()
        .load(std::sync::atomic::Ordering::Acquire)
}

#[tokio::test]
async fn modern_snapshot_identity_mismatch_closes_bootstrap_mesh_gate() {
    let tmp = tempfile::tempdir().unwrap();
    let reconcile = ReconcileHandle::noop();
    reconcile.initialize_mesh_gate(true).await;
    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap(),
    ));
    let state_machine = FileStateMachine::open(tmp.path(), store, reconcile.clone())
        .await
        .unwrap();
    state_machine.persist_meta().await.unwrap();

    let paths = StorePaths::new(tmp.path());
    let last_applied = LogId::new(openraft::CommittedLeaderId::new(1, 1), 7);
    let mut persisted: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&paths.sm_meta_json).unwrap()).unwrap();
    persisted["last_applied"] = serde_json::to_value(last_applied).unwrap();
    persisted["mesh_state_applied"] = json!(true);
    persisted["snapshot_install_pending"] = json!(false);
    std::fs::write(&paths.sm_meta_json, serde_json::to_vec(&persisted).unwrap()).unwrap();
    std::fs::write(
        &paths.snapshot_meta_json,
        serde_json::to_vec(&SnapshotMeta::<NodeId, NodeMeta> {
            last_log_id: Some(last_applied),
            last_membership: StoredMembership::default(),
            snapshot_id: "snapshot-7".to_string(),
        })
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        &paths.snapshot_data_json,
        serde_json::to_vec(&json!({
            "state": {},
            "mesh_state_applied": true,
            "snapshot_id": "snapshot-6",
            "last_log_id": last_applied,
        }))
        .unwrap(),
    )
    .unwrap();
    drop(state_machine);

    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap(),
    ));
    let mut restarted = FileStateMachine::open(tmp.path(), store, reconcile.clone())
        .await
        .unwrap();
    restarted.applied_state().await.unwrap();
    assert!(closed(&reconcile));
}

#[tokio::test]
async fn explicit_false_snapshot_marker_closes_bootstrap_mesh_gate_on_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let reconcile = ReconcileHandle::noop();
    reconcile.initialize_mesh_gate(true).await;
    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap(),
    ));
    let state_machine = FileStateMachine::open(tmp.path(), store, reconcile.clone())
        .await
        .unwrap();
    state_machine.persist_meta().await.unwrap();
    let paths = StorePaths::new(tmp.path());
    let mut persisted: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&paths.sm_meta_json).unwrap()).unwrap();
    persisted["mesh_state_applied"] = json!(false);
    persisted["snapshot_install_pending"] = json!(false);
    std::fs::write(&paths.sm_meta_json, serde_json::to_vec(&persisted).unwrap()).unwrap();
    drop(state_machine);

    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap(),
    ));
    let mut restarted = FileStateMachine::open(tmp.path(), store, reconcile.clone())
        .await
        .unwrap();
    restarted.applied_state().await.unwrap();
    assert!(closed(&reconcile));
}
