use super::*;
use crate::raft::{
    app::{BoxFuture, RaftFacade},
    types::{ClientResponse, NodeId, NodeMeta},
};
use crate::state::DesiredStateCommand;
use std::collections::BTreeSet;

struct RemovingRaft {
    inner: Arc<dyn RaftFacade>,
    state: AppState,
    removed: String,
    retained: String,
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
                store.state_mut().nodes.remove(&self.removed);
                store.save()?;
                drop(store);
                self.state
                    .node_history
                    .clear_node_with_destinations(
                        &self.removed,
                        std::slice::from_ref(&self.retained),
                    )
                    .await?;
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
