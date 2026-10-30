use super::*;
use crate::raft::{
    app::{BoxFuture, RaftFacade},
    types::{ClientResponse, NodeId, NodeMeta},
};
use crate::state::DesiredStateCommand;
use std::collections::BTreeSet;

fn removal_operation(node_id: &str) -> crate::state::MembershipOperation {
    crate::state::MembershipOperation {
        operation_id: "remove-interleaved".into(),
        kind: crate::state::MembershipOperationKind::RemoveNode,
        raft_node_id: 2,
        node_id: Some(node_id.into()),
        expected_membership: "test-membership".into(),
        phase: crate::state::MembershipOperationPhase::MembershipRemoved,
        legacy: false,
        remove_learner: false,
        delete_endpoints: true,
        expected_endpoint_ids: vec![],
        expected_endpoint_tags: vec![],
        created_at: "2026-01-01T00:00:00Z".into(),
        next_retry_at: None,
        terminal_at: None,
        evidence: Some("membership removed".into()),
    }
}

struct RemovingRaft {
    inner: Arc<dyn RaftFacade>,
    state: AppState,
    removed: String,
    retained: String,
    prune_before_return: bool,
}

impl RaftFacade for RemovingRaft {
    fn metrics(&self) -> tokio::sync::watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>> {
        self.inner.metrics()
    }

    fn client_write(
        &self,
        cmd: DesiredStateCommand,
    ) -> BoxFuture<'_, anyhow::Result<ClientResponse>> {
        Box::pin(async move {
            let deleting_user = matches!(&cmd, DesiredStateCommand::DeleteUser { .. });
            let result = self.inner.client_write(cmd).await?;
            if deleting_user {
                let mut store = self.state.store.lock().await;
                if !self.prune_before_return {
                    store.state_mut().membership_operations.insert(
                        "remove-interleaved".into(),
                        removal_operation(&self.removed),
                    );
                }
                store.state_mut().nodes.remove(&self.removed);
                store.save()?;
                drop(store);
                if self.prune_before_return {
                    self.state
                        .node_history
                        .clear_node_with_destinations(
                            &self.removed,
                            std::slice::from_ref(&self.retained),
                        )
                        .await?;
                }
            }
            Ok(result)
        })
    }

    fn add_learner(&self, id: NodeId, node: NodeMeta) -> BoxFuture<'_, anyhow::Result<()>> {
        self.inner.add_learner(id, node)
    }

    fn add_voters(&self, ids: BTreeSet<NodeId>) -> BoxFuture<'_, anyhow::Result<()>> {
        self.inner.add_voters(ids)
    }

    fn change_membership(
        &self,
        changes: openraft::ChangeMembers<NodeId, NodeMeta>,
        retain: bool,
    ) -> BoxFuture<'_, anyhow::Result<()>> {
        self.inner.change_membership(changes, retain)
    }
}

#[tokio::test]
async fn user_deletion_rechecks_membership_after_interleaved_node_removal() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = fixture::app_state(directory.path());
    let (user, removed, retained) = {
        let mut store = state.store.lock().await;
        let user = store
            .create_user("interleaved removal".to_owned(), None)
            .unwrap();
        let mut node = store.get_node(&state.cluster.node_id).unwrap();
        node.node_id = crate::id::new_ulid_string();
        let removed = node.node_id.clone();
        store.upsert_node(node.clone()).unwrap();
        node.node_id = crate::id::new_ulid_string();
        let retained = node.node_id.clone();
        store.upsert_node(node).unwrap();
        (user, removed, retained)
    };
    state.raft = Arc::new(RemovingRaft {
        inner: state.raft.clone(),
        state: state.clone(),
        removed: removed.clone(),
        retained: retained.clone(),
        prune_before_return: true,
    });
    assert_eq!(
        crate::http::admin_delete_user(
            axum::Extension(state.clone()),
            axum::extract::Path(user.user_id.clone()),
        )
        .await
        .unwrap(),
        axum::http::StatusCode::NO_CONTENT
    );

    // Only an actual repository ACK may retire the local marker.
    crate::http::history_repository::worker::publish_local_history_segment(
        &state,
        &[state.cluster.node_id.clone()],
        None,
        &[],
        110,
        true,
    )
    .await
    .unwrap();
    drop(state);
    let state = fixture::app_state(directory.path());
    let user_key = format!("node-history:user:{}:", user.user_id).into_bytes();
    let removed_markers = state
        .node_history
        .repository_deletion_markers(&removed)
        .await
        .unwrap();
    let retained_markers = state
        .node_history
        .repository_deletion_markers(&retained)
        .await
        .unwrap();
    assert!(
        !removed_markers
            .iter()
            .any(|marker| marker.record_key == user_key)
    );
    assert!(
        retained_markers
            .iter()
            .any(|marker| marker.record_key == user_key)
    );
}

#[tokio::test]
async fn committed_removal_is_not_reintroduced_before_cleanup_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = fixture::app_state(directory.path());
    let (user, removed, retained) = {
        let mut store = state.store.lock().await;
        let user = store
            .create_user("committed removal".to_owned(), None)
            .unwrap();
        let mut node = store.get_node(&state.cluster.node_id).unwrap();
        node.node_id = crate::id::new_ulid_string();
        let removed = node.node_id.clone();
        store.upsert_node(node.clone()).unwrap();
        node.node_id = crate::id::new_ulid_string();
        let retained = node.node_id.clone();
        store.upsert_node(node).unwrap();
        (user, removed, retained)
    };
    state.raft = Arc::new(RemovingRaft {
        inner: state.raft.clone(),
        state: state.clone(),
        removed: removed.clone(),
        retained: retained.clone(),
        prune_before_return: false,
    });
    crate::http::admin_delete_user(
        axum::Extension(state.clone()),
        axum::extract::Path(user.user_id.clone()),
    )
    .await
    .unwrap();
    crate::http::history_repository::worker::publish_local_history_segment(
        &state,
        &[state.cluster.node_id.clone()],
        None,
        &[],
        110,
        true,
    )
    .await
    .unwrap();
    drop(state);
    let state = fixture::app_state(directory.path());
    let key = format!("node-history:user:{}:", user.user_id).into_bytes();
    let removed_markers = state
        .node_history
        .repository_deletion_markers(&removed)
        .await
        .unwrap();
    let retained_markers = state
        .node_history
        .repository_deletion_markers(&retained)
        .await
        .unwrap();
    assert!(
        !removed_markers
            .iter()
            .any(|marker| marker.record_key == key)
    );
    assert!(
        retained_markers
            .iter()
            .any(|marker| marker.record_key == key)
    );
}

#[tokio::test]
async fn prepared_recovery_excludes_durably_removed_member_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let (user, removed, retained) = {
        let mut store = state.store.lock().await;
        let user = store.create_user("recover removal".into(), None).unwrap();
        let mut node = store.get_node(&state.cluster.node_id).unwrap();
        node.node_id = crate::id::new_ulid_string();
        let removed = node.node_id.clone();
        store.upsert_node(node.clone()).unwrap();
        node.node_id = crate::id::new_ulid_string();
        let retained = node.node_id.clone();
        store.upsert_node(node).unwrap();
        (user, removed, retained)
    };
    state
        .node_history
        .prepare_user_deletion(&user.user_id, &[removed.clone(), retained.clone()])
        .await
        .unwrap();
    {
        let mut store = state.store.lock().await;
        store
            .state_mut()
            .membership_operations
            .insert("remove-interleaved".into(), removal_operation(&removed));
        store.state_mut().nodes.remove(&removed);
        store.save().unwrap();
    }
    drop(state);
    let state = fixture::app_state(directory.path());
    assert_eq!(
        state
            .node_history
            .recover_prepared_user_deletions(
                &state.store,
                state.raft.as_ref(),
                &state.cluster.node_id,
            )
            .await
            .unwrap(),
        1
    );
    crate::http::history_repository::worker::publish_local_history_segment(
        &state,
        &[state.cluster.node_id.clone()],
        None,
        &[],
        110,
        true,
    )
    .await
    .unwrap();
    drop(state);
    let state = fixture::app_state(directory.path());
    let key = format!("node-history:user:{}:", user.user_id).into_bytes();
    assert!(
        !state
            .node_history
            .repository_deletion_markers(&removed)
            .await
            .unwrap()
            .iter()
            .any(|m| m.record_key == key)
    );
    assert!(
        state
            .node_history
            .repository_deletion_markers(&retained)
            .await
            .unwrap()
            .iter()
            .any(|m| m.record_key == key)
    );
}

#[tokio::test]
async fn unsaved_removal_never_discards_prepared_destination() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let removed = {
        let mut store = state.store.lock().await;
        let mut node = store.get_node(&state.cluster.node_id).unwrap();
        node.node_id = crate::id::new_ulid_string();
        let removed = node.node_id.clone();
        store.upsert_node(node).unwrap();
        store
            .state_mut()
            .membership_operations
            .insert("remove-interleaved".into(), removal_operation(&removed));
        store.save().unwrap();
        removed
    };
    state
        .node_history
        .prepare_user_deletion("target", std::slice::from_ref(&removed))
        .await
        .unwrap();
    {
        let mut store = state.store.lock().await;
        store.state_mut().nodes.remove(&removed);
        state
            .node_history
            .activate_user_deletion_for_membership("target", &store, &state.cluster.node_id)
            .await
            .unwrap();
    }
    crate::http::history_repository::worker::publish_local_history_segment(
        &state,
        &[state.cluster.node_id.clone()],
        None,
        &[],
        110,
        true,
    )
    .await
    .unwrap();
    drop(state);
    let state = fixture::app_state(directory.path());
    assert!(
        state
            .node_history
            .repository_deletion_markers(&removed)
            .await
            .unwrap()
            .iter()
            .any(|marker| marker.record_key == b"node-history:user:target:")
    );
}

#[tokio::test]
async fn unreadable_removal_proof_preserves_preparation_for_retry() {
    let directory = tempfile::tempdir().unwrap();
    let state = fixture::app_state(directory.path());
    let removed = {
        let mut store = state.store.lock().await;
        let mut node = store.get_node(&state.cluster.node_id).unwrap();
        node.node_id = crate::id::new_ulid_string();
        let removed = node.node_id.clone();
        store.upsert_node(node).unwrap();
        removed
    };
    state
        .node_history
        .prepare_user_deletion("target", std::slice::from_ref(&removed))
        .await
        .unwrap();
    {
        let mut store = state.store.lock().await;
        store
            .state_mut()
            .membership_operations
            .insert("remove-interleaved".into(), removal_operation(&removed));
        store.state_mut().nodes.remove(&removed);
        store.save().unwrap();
    }
    let connection = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    let original: Vec<u8> = connection
        .query_row(
            "SELECT payload FROM history_snapshots WHERE key = 'persistent_state'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    connection
        .execute(
            "UPDATE history_snapshots SET payload = ?1 WHERE key = 'persistent_state'",
            [b"bad-json".as_slice()],
        )
        .unwrap();
    {
        let store = state.store.lock().await;
        assert!(
            state
                .node_history
                .activate_user_deletion_for_membership("target", &store, &state.cluster.node_id)
                .await
                .is_err()
        );
    }
    assert!(
        state
            .node_history
            .repository_deletion_markers("local")
            .await
            .unwrap()
            .is_empty()
    );
    connection
        .execute(
            "UPDATE history_snapshots SET payload = ?1 WHERE key = 'persistent_state'",
            [original],
        )
        .unwrap();
    {
        let store = state.store.lock().await;
        state
            .node_history
            .activate_user_deletion_for_membership("target", &store, &state.cluster.node_id)
            .await
            .unwrap();
    }
    assert!(
        state
            .node_history
            .repository_deletion_markers("local")
            .await
            .unwrap()
            .iter()
            .any(|marker| marker.record_key == b"node-history:user:target:")
    );
}
