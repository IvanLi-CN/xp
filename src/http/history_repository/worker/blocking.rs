use std::sync::Arc;

use tokio::sync::Mutex;

use crate::state::history_repository::replica::RepositoryReplicaRuntime;

pub(super) fn repository_blocking<T>(operation: impl FnOnce() -> T) -> T {
    if tokio::runtime::Handle::current().runtime_flavor()
        == tokio::runtime::RuntimeFlavor::MultiThread
    {
        tokio::task::block_in_place(operation)
    } else {
        operation()
    }
}

pub(super) async fn repository_op<T: Send>(
    replica: &Arc<Mutex<RepositoryReplicaRuntime>>,
    operation: impl FnOnce(&mut RepositoryReplicaRuntime) -> T + Send,
) -> T {
    let mut runtime = replica.lock().await;
    repository_blocking(|| operation(&mut runtime))
}
