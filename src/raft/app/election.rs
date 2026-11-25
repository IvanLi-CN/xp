use std::time::Duration;

use tokio::sync::watch;

use super::catch_raft_panic;
use crate::raft::types::{NodeId, NodeMeta, TypeConfig};

pub(super) async fn elect_local_raft(
    raft: &openraft::Raft<TypeConfig>,
    metrics: &watch::Receiver<openraft::RaftMetrics<NodeId, NodeMeta>>,
) -> anyhow::Result<()> {
    let local_id = metrics.borrow().id;
    {
        let snapshot = metrics.borrow();
        if snapshot.state == openraft::ServerState::Leader
            && snapshot.current_leader == Some(local_id)
        {
            return Ok(());
        }
    }

    catch_raft_panic("raft trigger elect", raft.trigger().elect())
        .await?
        .map_err(|error| anyhow::anyhow!("raft trigger elect: {error}"))?;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut rx = metrics.clone();
    loop {
        {
            let snapshot = rx.borrow();
            if snapshot.state == openraft::ServerState::Leader
                && snapshot.current_leader == Some(local_id)
            {
                return Ok(());
            }
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            anyhow::bail!("timed out waiting for local leader election");
        }
        tokio::time::timeout(deadline - now, rx.changed())
            .await
            .map_err(|_| anyhow::anyhow!("timed out waiting for local leader election"))?
            .map_err(|error| anyhow::anyhow!("raft metrics channel closed: {error}"))?;
    }
}
