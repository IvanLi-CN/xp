use std::sync::Arc;

use tokio::sync::Mutex;

use crate::state::history_repository::replica::{RepositoryReplicaRuntime, RepositoryRuntimeError};

pub(super) async fn repository_blocking<T: Send + 'static>(
    replica: Arc<Mutex<RepositoryReplicaRuntime>>,
    operation: impl FnOnce(&mut RepositoryReplicaRuntime) -> Result<T, RepositoryRuntimeError>
    + Send
    + 'static,
) -> anyhow::Result<T> {
    tokio::task::spawn_blocking(move || operation(&mut replica.blocking_lock()))
        .await
        .map_err(|error| anyhow::anyhow!("join repository storage work: {error}"))?
        .map_err(Into::into)
}
