use std::{path::Path, sync::Arc, time::Duration};

use super::*;
use crate::{
    raft::types::TypeConfig,
    reconcile::ReconcileHandle,
    state::{DesiredStateCommand, JsonSnapshotStore, StoreInit},
};
use openraft::{EntryPayload, LogId};
use tokio::sync::Mutex;

fn test_store_init(tmp_dir: &Path) -> StoreInit {
    StoreInit {
        data_dir: tmp_dir.to_path_buf(),
        bootstrap_node_id: None,
        bootstrap_node_name: xp_test_fixtures::label_node1_variant2().to_owned(),
        bootstrap_access_host: xp_test_fixtures::label_empty().to_owned(),
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

#[tokio::test]
async fn explicit_mesh_switch_apply_is_bounded_by_mesh_body_guard() {
    let tmp = tempfile::tempdir().unwrap();
    let reconcile = ReconcileHandle::noop();
    let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
    let store = Arc::new(Mutex::new(store));
    let mut state_machine = FileStateMachine::open(tmp.path(), store, reconcile.clone())
        .await
        .unwrap();
    let gate_lock = reconcile.mesh_gate_lock();
    let in_flight_mesh_read = gate_lock.clone().read_owned().await;
    let state_machine_inner = state_machine.inner.clone();

    let mut apply = tokio::spawn(async move {
        state_machine
            .apply(vec![build_entry(
                DesiredStateCommand::SetMeshEnabled { enabled: false },
                1,
            )])
            .await
    });
    tokio::time::timeout(Duration::from_secs(4), &mut apply)
        .await
        .expect("explicit Mesh switch must not wait for a long-lived body guard")
        .expect("state-machine task should not panic")
        .expect("state-machine apply should succeed");
    assert_eq!(
        state_machine_inner
            .lock()
            .await
            .last_applied
            .expect("apply should advance last_applied")
            .index,
        1
    );
    assert!(
        !reconcile
            .mesh_gate()
            .load(std::sync::atomic::Ordering::Acquire),
        "a timed-out disable must fail closed"
    );

    drop(in_flight_mesh_read);
}

#[tokio::test]
async fn snapshot_mesh_gate_hold_is_bounded_by_its_deadline() {
    let reconcile = ReconcileHandle::noop();
    let in_flight_mesh_read = reconcile.mesh_gate_lock().read_owned().await;

    let started = std::time::Instant::now();
    assert!(
        !reconcile
            .hold_mesh_gate_until_raft_state_until(started + Duration::from_millis(50))
            .await
    );
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "snapshot gate admission must not wait behind an in-flight body forever"
    );

    drop(in_flight_mesh_read);
}

#[tokio::test]
async fn initial_mesh_gate_apply_is_bounded_by_mesh_body_guard() {
    let tmp = tempfile::tempdir().unwrap();
    let reconcile = ReconcileHandle::noop();
    reconcile.hold_mesh_gate_until_raft_state().await;
    let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
    let store = Arc::new(Mutex::new(store));
    let mut state_machine = FileStateMachine::open(tmp.path(), store, reconcile.clone())
        .await
        .unwrap();
    let gate_lock = reconcile.mesh_gate_lock();
    let in_flight_mesh_read = gate_lock.clone().read_owned().await;
    let state_machine_inner = state_machine.inner.clone();

    let mut apply = tokio::spawn(async move {
        state_machine
            .apply(vec![build_entry(
                DesiredStateCommand::SetReverseMeshEpoch { epoch: 1 },
                1,
            )])
            .await
    });
    tokio::time::timeout(Duration::from_secs(4), &mut apply)
        .await
        .expect("initial Mesh gate publication must be bounded")
        .expect("state-machine task should not panic")
        .expect("state-machine apply should succeed");
    assert_eq!(
        state_machine_inner
            .lock()
            .await
            .last_applied
            .expect("apply should advance last_applied")
            .index,
        1
    );
    assert!(
        !reconcile
            .mesh_gate()
            .load(std::sync::atomic::Ordering::Acquire)
    );

    drop(in_flight_mesh_read);
}
