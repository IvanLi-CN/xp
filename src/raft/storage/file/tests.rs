use std::{path::Path, sync::Arc};

use openraft::RaftSnapshotBuilder;
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
        bootstrap_node_name: xp_test_fixtures::label_node1_variant2().to_owned(),
        bootstrap_access_host: "".to_string(),
        bootstrap_api_base_url: xp_test_fixtures::subscription_api_loopback_https().to_owned(),
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
                    "reality": xp_test_fixtures::endpoint_reality()
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

    let entry = build_entry(
        DesiredStateCommand::UpsertEndpoint {
            endpoint,
            expected: None,
        },
        1,
    );
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
async fn apply_mesh_switch_publishes_gate_before_reconcile_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let reconcile = ReconcileHandle::noop();
    let gate = reconcile.mesh_gate();
    let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
    let store = Arc::new(Mutex::new(store));
    let mut state_machine = FileStateMachine::open(tmp.path(), store.clone(), reconcile)
        .await
        .unwrap();

    assert!(gate.load(std::sync::atomic::Ordering::Acquire));
    state_machine
        .apply(vec![build_entry(
            DesiredStateCommand::SetMeshEnabled { enabled: false },
            1,
        )])
        .await
        .unwrap();

    assert!(!gate.load(std::sync::atomic::Ordering::Acquire));
    assert!(!store.lock().await.state().mesh_enabled);
}

#[tokio::test]
async fn ordinary_command_releases_fresh_join_mesh_gate_once_authenticated() {
    let tmp = tempfile::tempdir().unwrap();
    let reconcile = ReconcileHandle::noop();
    reconcile.hold_mesh_gate_until_raft_state().await;
    let gate = reconcile.mesh_gate();
    let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
    let store = Arc::new(Mutex::new(store));
    let mut state_machine = FileStateMachine::open(tmp.path(), store, reconcile)
        .await
        .unwrap();

    state_machine
        .apply(vec![build_entry(
            DesiredStateCommand::SetReverseMeshEpoch { epoch: 1 },
            1,
        )])
        .await
        .unwrap();

    assert!(gate.load(std::sync::atomic::Ordering::Acquire));

    state_machine
        .apply(vec![build_entry(
            DesiredStateCommand::SetMeshEnabled { enabled: false },
            2,
        )])
        .await
        .unwrap();
    assert!(!gate.load(std::sync::atomic::Ordering::Acquire));
}

#[tokio::test]
async fn rejected_normal_command_still_releases_fresh_join_mesh_gate() {
    let tmp = tempfile::tempdir().unwrap();
    let reconcile = ReconcileHandle::noop();
    reconcile.hold_mesh_gate_until_raft_state().await;
    let gate = reconcile.mesh_gate();
    let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
    let store = Arc::new(Mutex::new(store));
    let node_id = store.lock().await.list_nodes()[0].node_id.clone();
    store
        .lock()
        .await
        .create_endpoint(
            node_id.clone(),
            EndpointKind::VlessRealityVisionTcp,
            443,
            json!({"reality": xp_test_fixtures::endpoint_reality()}),
        )
        .unwrap();
    let mut state_machine = FileStateMachine::open(tmp.path(), store, reconcile.clone())
        .await
        .unwrap();

    let responses = state_machine
        .apply(vec![build_entry(
            DesiredStateCommand::DeleteNode {
                node_id,
                delete_endpoints: false,
                expected_endpoint_ids: Vec::new(),
                join_session: None,
            },
            1,
        )])
        .await
        .unwrap();

    assert!(matches!(
        responses.as_slice(),
        [ClientResponse::Err { status: 409, .. }]
    ));
    assert!(gate.load(std::sync::atomic::Ordering::Acquire));
}

#[tokio::test]
async fn applied_state_releases_mesh_gate_after_a_caught_up_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let reconcile = ReconcileHandle::noop();
    let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
    let store = Arc::new(Mutex::new(store));
    let mut state_machine = FileStateMachine::open(tmp.path(), store.clone(), reconcile.clone())
        .await
        .unwrap();
    state_machine
        .apply(vec![build_entry(
            DesiredStateCommand::SetReverseMeshEpoch { epoch: 1 },
            1,
        )])
        .await
        .unwrap();
    reconcile.hold_mesh_gate_until_raft_state().await;
    drop(state_machine);

    let mut restarted = FileStateMachine::open(tmp.path(), store, reconcile.clone())
        .await
        .unwrap();
    restarted.applied_state().await.unwrap();
    assert!(
        reconcile
            .mesh_gate()
            .load(std::sync::atomic::Ordering::Acquire)
    );
}

#[tokio::test]
async fn membership_and_blank_entries_keep_a_fresh_join_mesh_gate_closed() {
    for payload in [
        EntryPayload::Blank,
        EntryPayload::Membership(openraft::Membership::new(
            vec![std::collections::BTreeSet::from([1])],
            std::collections::BTreeMap::new(),
        )),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let reconcile = ReconcileHandle::noop();
        reconcile.hold_mesh_gate_until_raft_state().await;
        let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
        let store = Arc::new(Mutex::new(store));
        let mut state_machine = FileStateMachine::open(tmp.path(), store, reconcile.clone())
            .await
            .unwrap();
        state_machine
            .apply(vec![openraft::impls::Entry {
                log_id: LogId::new(openraft::CommittedLeaderId::new(1, 1), 1),
                payload,
            }])
            .await
            .unwrap();
        assert!(
            !reconcile
                .mesh_gate()
                .load(std::sync::atomic::Ordering::Acquire)
        );
    }
}

#[tokio::test]
async fn membership_then_state_entry_releases_a_fresh_join_mesh_gate() {
    let tmp = tempfile::tempdir().unwrap();
    let reconcile = ReconcileHandle::noop();
    reconcile.hold_mesh_gate_until_raft_state().await;
    let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
    let store = Arc::new(Mutex::new(store));
    let mut state_machine = FileStateMachine::open(tmp.path(), store, reconcile.clone())
        .await
        .unwrap();
    state_machine
        .apply(vec![openraft::impls::Entry {
            log_id: LogId::new(openraft::CommittedLeaderId::new(1, 1), 1),
            payload: EntryPayload::Membership(openraft::Membership::new(
                vec![std::collections::BTreeSet::from([1])],
                std::collections::BTreeMap::new(),
            )),
        }])
        .await
        .unwrap();
    assert!(
        !reconcile
            .mesh_gate()
            .load(std::sync::atomic::Ordering::Acquire)
    );
    state_machine
        .apply(vec![build_entry(
            DesiredStateCommand::SetReverseMeshEpoch { epoch: 1 },
            2,
        )])
        .await
        .unwrap();
    assert!(
        reconcile
            .mesh_gate()
            .load(std::sync::atomic::Ordering::Acquire)
    );
}

#[tokio::test]
async fn legacy_state_machine_meta_keeps_membership_only_restart_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let reconcile = ReconcileHandle::noop();
    reconcile.hold_mesh_gate_until_raft_state().await;
    let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
    let store = Arc::new(Mutex::new(store));
    let mut state_machine = FileStateMachine::open(tmp.path(), store.clone(), reconcile.clone())
        .await
        .unwrap();
    state_machine
        .apply(vec![openraft::impls::Entry {
            log_id: LogId::new(openraft::CommittedLeaderId::new(1, 1), 1),
            payload: EntryPayload::Membership(openraft::Membership::new(
                vec![std::collections::BTreeSet::from([1])],
                std::collections::BTreeMap::new(),
            )),
        }])
        .await
        .unwrap();
    let paths = StorePaths::new(tmp.path());
    let mut meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&paths.sm_meta_json).unwrap()).unwrap();
    meta.as_object_mut().unwrap().remove("mesh_state_applied");
    std::fs::write(&paths.sm_meta_json, serde_json::to_vec(&meta).unwrap()).unwrap();
    drop(state_machine);

    let restarted = FileStateMachine::open(tmp.path(), store, reconcile.clone())
        .await
        .unwrap();
    let mut restarted = restarted;
    restarted.applied_state().await.unwrap();
    assert!(
        !reconcile
            .mesh_gate()
            .load(std::sync::atomic::Ordering::Acquire)
    );
}

#[tokio::test]
async fn legacy_state_machine_meta_keeps_blank_only_restart_closed() {
    for with_membership in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let reconcile = ReconcileHandle::noop();
        reconcile.hold_mesh_gate_until_raft_state().await;
        let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
        let store = Arc::new(Mutex::new(store));
        let mut state_machine =
            FileStateMachine::open(tmp.path(), store.clone(), reconcile.clone())
                .await
                .unwrap();
        if with_membership {
            state_machine
                .apply(vec![openraft::impls::Entry {
                    log_id: LogId::new(openraft::CommittedLeaderId::new(1, 1), 1),
                    payload: EntryPayload::Membership(openraft::Membership::new(
                        vec![std::collections::BTreeSet::from([1])],
                        std::collections::BTreeMap::new(),
                    )),
                }])
                .await
                .unwrap();
        }
        state_machine
            .apply(vec![openraft::impls::Entry {
                log_id: LogId::new(openraft::CommittedLeaderId::new(1, 1), 2),
                payload: EntryPayload::Blank,
            }])
            .await
            .unwrap();
        let paths = StorePaths::new(tmp.path());
        let mut meta: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&paths.sm_meta_json).unwrap()).unwrap();
        meta.as_object_mut().unwrap().remove("mesh_state_applied");
        std::fs::write(&paths.sm_meta_json, serde_json::to_vec(&meta).unwrap()).unwrap();
        std::fs::write(
            &paths.snapshot_meta_json,
            serde_json::to_vec(&SnapshotMeta::<NodeId, NodeMeta> {
                last_log_id: Some(LogId::new(openraft::CommittedLeaderId::new(1, 1), 2)),
                last_membership: StoredMembership::default(),
                snapshot_id: "local-build-only".to_string(),
            })
            .unwrap(),
        )
        .unwrap();
        drop(state_machine);

        let mut restarted = FileStateMachine::open(tmp.path(), store, reconcile.clone())
            .await
            .unwrap();
        restarted.applied_state().await.unwrap();
        assert!(
            !reconcile
                .mesh_gate()
                .load(std::sync::atomic::Ordering::Acquire)
        );
    }
}

#[tokio::test]
async fn legacy_state_machine_meta_keeps_wal_only_restart_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let reconcile = ReconcileHandle::noop();
    reconcile.hold_mesh_gate_until_raft_state().await;
    let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
    let store = Arc::new(Mutex::new(store));
    let mut state_machine = FileStateMachine::open(tmp.path(), store.clone(), reconcile.clone())
        .await
        .unwrap();
    state_machine
        .apply(vec![openraft::impls::Entry {
            log_id: LogId::new(openraft::CommittedLeaderId::new(1, 1), 1),
            payload: EntryPayload::Membership(openraft::Membership::new(
                vec![std::collections::BTreeSet::from([1])],
                std::collections::BTreeMap::new(),
            )),
        }])
        .await
        .unwrap();
    state_machine
        .apply(vec![build_entry(
            DesiredStateCommand::SetReverseMeshEpoch { epoch: 1 },
            2,
        )])
        .await
        .unwrap();
    let paths = StorePaths::new(tmp.path());
    std::fs::write(
        &paths.wal_json,
        serde_json::to_vec(&PersistedWal {
            last_purged_log_id: None,
            entries: vec![build_entry(
                DesiredStateCommand::SetReverseMeshEpoch { epoch: 1 },
                2,
            )],
        })
        .unwrap(),
    )
    .unwrap();
    let mut meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&paths.sm_meta_json).unwrap()).unwrap();
    meta.as_object_mut().unwrap().remove("mesh_state_applied");
    std::fs::write(&paths.sm_meta_json, serde_json::to_vec(&meta).unwrap()).unwrap();
    reconcile.hold_mesh_gate_until_raft_state().await;
    drop(state_machine);

    let mut restarted = FileStateMachine::open(tmp.path(), store, reconcile.clone())
        .await
        .unwrap();
    restarted.applied_state().await.unwrap();
    assert!(
        !reconcile
            .mesh_gate()
            .load(std::sync::atomic::Ordering::Acquire)
    );
}

#[tokio::test]
async fn legacy_state_machine_meta_reopens_from_authenticated_snapshot_after_wal_purge() {
    let tmp = tempfile::tempdir().unwrap();
    let reconcile = ReconcileHandle::noop();
    reconcile.hold_mesh_gate_until_raft_state().await;
    let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
    let store = Arc::new(Mutex::new(store));
    let mut state_machine = FileStateMachine::open(tmp.path(), store.clone(), reconcile.clone())
        .await
        .unwrap();
    state_machine
        .apply(vec![build_entry(
            DesiredStateCommand::SetReverseMeshEpoch { epoch: 1 },
            2,
        )])
        .await
        .unwrap();

    let mut builder = state_machine.get_snapshot_builder().await;
    builder.build_snapshot().await.unwrap();
    let paths = StorePaths::new(tmp.path());
    std::fs::write(
        &paths.wal_json,
        serde_json::to_vec(&PersistedWal {
            last_purged_log_id: Some(LogId::new(openraft::CommittedLeaderId::new(1, 1), 2)),
            entries: Vec::new(),
        })
        .unwrap(),
    )
    .unwrap();
    let mut meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&paths.sm_meta_json).unwrap()).unwrap();
    meta.as_object_mut().unwrap().remove("mesh_state_applied");
    std::fs::write(&paths.sm_meta_json, serde_json::to_vec(&meta).unwrap()).unwrap();
    drop(state_machine);

    let mut restarted = FileStateMachine::open(tmp.path(), store, reconcile.clone())
        .await
        .unwrap();
    restarted.applied_state().await.unwrap();
    assert!(
        reconcile
            .mesh_gate()
            .load(std::sync::atomic::Ordering::Acquire)
    );
}

#[tokio::test]
async fn legacy_state_machine_meta_keeps_blank_snapshot_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let reconcile = ReconcileHandle::noop();
    reconcile.hold_mesh_gate_until_raft_state().await;
    let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
    let store = Arc::new(Mutex::new(store));
    let mut state_machine = FileStateMachine::open(tmp.path(), store.clone(), reconcile.clone())
        .await
        .unwrap();
    state_machine
        .apply(vec![openraft::impls::Entry {
            log_id: LogId::new(openraft::CommittedLeaderId::new(1, 1), 2),
            payload: EntryPayload::Blank,
        }])
        .await
        .unwrap();
    let mut builder = state_machine.get_snapshot_builder().await;
    builder.build_snapshot().await.unwrap();
    let paths = StorePaths::new(tmp.path());
    std::fs::write(
        &paths.wal_json,
        serde_json::to_vec(&PersistedWal {
            last_purged_log_id: Some(LogId::new(openraft::CommittedLeaderId::new(1, 1), 2)),
            entries: Vec::new(),
        })
        .unwrap(),
    )
    .unwrap();
    let mut meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&paths.sm_meta_json).unwrap()).unwrap();
    meta.as_object_mut().unwrap().remove("mesh_state_applied");
    std::fs::write(&paths.sm_meta_json, serde_json::to_vec(&meta).unwrap()).unwrap();
    drop(state_machine);

    let mut restarted = FileStateMachine::open(tmp.path(), store, reconcile.clone())
        .await
        .unwrap();
    restarted.applied_state().await.unwrap();
    assert!(
        !reconcile
            .mesh_gate()
            .load(std::sync::atomic::Ordering::Acquire)
    );
}

#[test]
fn snapshot_payload_metadata_mismatch_is_rejected() {
    let meta = SnapshotMeta::<NodeId, NodeMeta> {
        last_log_id: Some(LogId::new(openraft::CommittedLeaderId::new(2, 7), 11)),
        last_membership: StoredMembership::default(),
        snapshot_id: "snapshot-11".to_string(),
    };
    let payload = serde_json::json!({
        "state": {},
        "mesh_state_applied": true,
        "snapshot_id": "snapshot-10",
        "last_log_id": meta.last_log_id,
    });
    let bytes = serde_json::to_vec(&payload).unwrap();
    assert!(super::legacy_mesh::validate_snapshot_payload(&meta, &bytes).is_err());
}

#[test]
fn authenticated_snapshot_requires_identity_fields() {
    let meta = SnapshotMeta::<NodeId, NodeMeta> {
        last_log_id: None,
        last_membership: StoredMembership::default(),
        snapshot_id: "snapshot-authenticated".to_string(),
    };
    let bytes = serde_json::to_vec(&json!({
        "state": {},
        "mesh_state_applied": true,
    }))
    .unwrap();
    assert!(super::legacy_mesh::validate_snapshot_payload(&meta, &bytes).is_err());
}

#[tokio::test]
async fn install_snapshot_with_false_marker_keeps_mesh_gate_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let reconcile = ReconcileHandle::noop();
    let gate = reconcile.mesh_gate();
    let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
    let store = Arc::new(Mutex::new(store));
    let mut snapshot_state = store.lock().await.state().clone();
    snapshot_state.mesh_enabled = true;
    let meta = SnapshotMeta {
        last_log_id: None,
        last_membership: StoredMembership::default(),
        snapshot_id: "snapshot-false-marker".to_string(),
    };
    let bytes = serde_json::to_vec(&json!({
        "state": snapshot_state,
        "mesh_state_applied": false,
        "snapshot_id": meta.snapshot_id,
        "last_log_id": meta.last_log_id,
    }))
    .unwrap();
    let mut state_machine = FileStateMachine::open(tmp.path(), store, reconcile.clone())
        .await
        .unwrap();

    state_machine
        .install_snapshot(&meta, Box::new(std::io::Cursor::new(bytes)))
        .await
        .unwrap();

    assert!(!gate.load(std::sync::atomic::Ordering::Acquire));
    let persisted: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&StorePaths::new(tmp.path()).sm_meta_json).unwrap())
            .unwrap();
    assert_eq!(persisted["mesh_state_applied"], false);
    assert_eq!(persisted["snapshot_install_pending"], false);
}

#[tokio::test]
async fn pending_snapshot_install_keeps_restart_mesh_gate_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let reconcile = ReconcileHandle::noop();
    reconcile.hold_mesh_gate_until_raft_state().await;
    let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
    let store = Arc::new(Mutex::new(store));
    let state_machine = FileStateMachine::open(tmp.path(), store.clone(), reconcile.clone())
        .await
        .unwrap();
    state_machine
        .persist_meta()
        .await
        .expect("write baseline state machine metadata");
    let paths = StorePaths::new(tmp.path());
    let mut persisted: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&paths.sm_meta_json).unwrap()).unwrap();
    persisted["mesh_state_applied"] = json!(true);
    persisted["snapshot_install_pending"] = json!(true);
    std::fs::write(&paths.sm_meta_json, serde_json::to_vec(&persisted).unwrap()).unwrap();
    drop(state_machine);

    let mut restarted = FileStateMachine::open(tmp.path(), store, reconcile.clone())
        .await
        .unwrap();
    restarted.applied_state().await.unwrap();
    assert!(
        !reconcile
            .mesh_gate()
            .load(std::sync::atomic::Ordering::Acquire)
    );
}

#[tokio::test]
async fn normal_apply_clears_pending_snapshot_marker_for_later_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let reconcile = ReconcileHandle::noop();
    reconcile.hold_mesh_gate_until_raft_state().await;
    let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
    let store = Arc::new(Mutex::new(store));
    let state_machine = FileStateMachine::open(tmp.path(), store.clone(), reconcile.clone())
        .await
        .unwrap();
    state_machine.persist_meta().await.unwrap();
    let paths = StorePaths::new(tmp.path());
    let mut persisted: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&paths.sm_meta_json).unwrap()).unwrap();
    persisted["mesh_state_applied"] = json!(true);
    persisted["snapshot_install_pending"] = json!(true);
    std::fs::write(&paths.sm_meta_json, serde_json::to_vec(&persisted).unwrap()).unwrap();
    drop(state_machine);

    let mut restarted = FileStateMachine::open(tmp.path(), store.clone(), reconcile.clone())
        .await
        .unwrap();
    restarted
        .apply(vec![build_entry(
            DesiredStateCommand::SetReverseMeshEpoch { epoch: 1 },
            1,
        )])
        .await
        .unwrap();
    drop(restarted);

    let mut reopened = FileStateMachine::open(tmp.path(), store, reconcile.clone())
        .await
        .unwrap();
    reopened.applied_state().await.unwrap();
    assert!(
        reconcile
            .mesh_gate()
            .load(std::sync::atomic::Ordering::Acquire)
    );
    let persisted: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&paths.sm_meta_json).unwrap()).unwrap();
    assert_eq!(persisted["snapshot_install_pending"], false);
}

#[tokio::test]
async fn install_snapshot_publishes_mesh_gate_before_reconcile_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let reconcile = ReconcileHandle::noop();
    let gate = reconcile.mesh_gate();
    let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
    let store = Arc::new(Mutex::new(store));
    let mut snapshot_state = store.lock().await.state().clone();
    snapshot_state.mesh_enabled = false;
    let bytes = serde_json::to_vec(&json!({ "state": snapshot_state })).unwrap();
    let mut state_machine = FileStateMachine::open(tmp.path(), store.clone(), reconcile)
        .await
        .unwrap();
    let meta = SnapshotMeta {
        last_log_id: None,
        last_membership: StoredMembership::default(),
        snapshot_id: "snapshot-mesh-gate".to_string(),
    };

    assert!(gate.load(std::sync::atomic::Ordering::Acquire));
    state_machine
        .install_snapshot(&meta, Box::new(std::io::Cursor::new(bytes)))
        .await
        .unwrap();

    assert!(!gate.load(std::sync::atomic::Ordering::Acquire));
    assert!(!store.lock().await.state().mesh_enabled);
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
        node_id: xp_test_fixtures::label_node1().to_owned(),
        node_name: xp_test_fixtures::label_node1_variant2().to_owned(),
        access_host: xp_test_fixtures::host_fixture465().to_owned(),
        api_base_url: xp_test_fixtures::url_loopback62416().to_owned(),
        quota_limit_bytes: 0,
        quota_reset: NodeQuotaReset::default(),
    };
    let endpoint = Endpoint {
        endpoint_id: xp_test_fixtures::label_endpoint1().to_owned(),
        node_id: xp_test_fixtures::label_node1().to_owned(),
        tag: xp_test_fixtures::endpoint_tag_fixture480().to_owned(),
        kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
        port: 8388,
        meta: json!({}),
    };
    let user = User {
        user_id: "user_1".to_string(),
        display_name: "alice".to_string(),
        subscription_token: xp_test_fixtures::label_sub1().to_owned(),
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
async fn install_snapshot_blocks_reverse_mesh_schema_rollback() {
    let tmp = tempfile::tempdir().unwrap();
    let reconcile = ReconcileHandle::noop();
    let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
    let store = Arc::new(Mutex::new(store));
    {
        let mut guard = store.lock().await;
        guard.state_mut().reverse_mesh_epoch = 7;
    }
    let mut state_machine = FileStateMachine::open(tmp.path(), store.clone(), reconcile)
        .await
        .unwrap();
    let mut old_state = store.lock().await.state().clone();
    old_state.schema_version = crate::state::SCHEMA_VERSION - 1;
    old_state.reverse_mesh_epoch = 0;
    let bytes = serde_json::to_vec(&json!({ "state": old_state })).unwrap();
    let meta = SnapshotMeta {
        last_log_id: None,
        last_membership: StoredMembership::default(),
        snapshot_id: "snapshot-reverse-rollback".to_string(),
    };

    let error = state_machine
        .install_snapshot(&meta, Box::new(std::io::Cursor::new(bytes)))
        .await
        .expect_err("old schema must not replace an active reverse epoch");
    assert!(error.to_string().contains("schema rollback is blocked"));
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
