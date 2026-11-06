use super::*;
use crate::http::history_repository::derived_repository_identity;
use crate::state::history_repository::control::{
    RepositoryCapacity, RepositoryMember, RepositoryMembership,
};
#[path = "../backfill/tests/app_state.rs"]
mod fixture;

#[tokio::test]
async fn local_quota_does_not_generate_noop_raft_usage_writes() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = fixture::app_state(directory.path());
    let local_id = RepositoryNodeId::try_from(state.cluster.node_id.clone()).unwrap();
    let identity = derived_repository_identity(&state, local_id.clone()).unwrap();
    let mut advertised = RepositoryCapacity::default();
    advertised.record_usage(123, 1_000_000_000).unwrap();
    let member = RepositoryMember::new(identity, advertised).unwrap();
    state.store.lock().await.state_mut().repository_membership =
        Some(RepositoryMembership::new(vec![member]).unwrap());
    {
        let mut runtime = state.repository_replica.lock().await;
        runtime.force_capacity_for_test(123, 1_000_000_000).unwrap();
        runtime.set_repository_quota_bytes(9_000_000_000).unwrap();
    }
    // Reject every write so success proves that the usage-only patch was not sent.
    state.raft = std::sync::Arc::new(RejectWrites(state.raft.clone()));
    sync_local_repository_capacity(&state)
        .await
        .expect("unchanged usage needs no leader write");
    state
        .repository_replica
        .lock()
        .await
        .force_capacity_for_test(124, 1_000_000_000)
        .unwrap();
    assert!(
        sync_local_repository_capacity(&state).await.is_err(),
        "changed usage must still require a real Raft write"
    );
    let store = state.store.lock().await;
    let member = store
        .state()
        .repository_membership
        .as_ref()
        .unwrap()
        .repository(&local_id)
        .unwrap();
    assert_eq!(member.capacity().quota_bytes(), 10 * 1024 * 1024 * 1024);
    assert_eq!(member.capacity().used_bytes(), 123);
}

struct RejectWrites(std::sync::Arc<dyn crate::raft::app::RaftFacade>);
impl crate::raft::app::RaftFacade for RejectWrites {
    fn metrics(
        &self,
    ) -> tokio::sync::watch::Receiver<openraft::RaftMetrics<u64, crate::raft::types::NodeMeta>>
    {
        self.0.metrics()
    }
    fn client_write(
        &self,
        _: crate::state::DesiredStateCommand,
    ) -> crate::raft::app::BoxFuture<'_, anyhow::Result<crate::raft::types::ClientResponse>> {
        Box::pin(async { anyhow::bail!("unexpected usage-only write") })
    }
    fn add_learner(
        &self,
        id: u64,
        node: crate::raft::types::NodeMeta,
    ) -> crate::raft::app::BoxFuture<'_, anyhow::Result<()>> {
        self.0.add_learner(id, node)
    }
    fn add_voters(
        &self,
        ids: std::collections::BTreeSet<u64>,
    ) -> crate::raft::app::BoxFuture<'_, anyhow::Result<()>> {
        self.0.add_voters(ids)
    }
    fn change_membership(
        &self,
        changes: openraft::ChangeMembers<u64, crate::raft::types::NodeMeta>,
        retain: bool,
    ) -> crate::raft::app::BoxFuture<'_, anyhow::Result<()>> {
        self.0.change_membership(changes, retain)
    }
}
