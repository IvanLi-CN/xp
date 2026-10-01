use std::{future::Future, pin::Pin};

use futures_util::{StreamExt, stream::FuturesUnordered};
use tokio::sync::mpsc;

use super::MeshAwareHttpClient;

type Completion = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

const COMPLETION_QUEUE_CAPACITY: usize = 256;

#[derive(Clone)]
pub(super) struct CompletionDispatcher {
    sender: mpsc::Sender<Completion>,
    #[cfg(test)]
    worker_starts: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl CompletionDispatcher {
    pub(super) fn new() -> Self {
        let (sender, mut receiver) = mpsc::channel(COMPLETION_QUEUE_CAPACITY);
        #[cfg(test)]
        let worker_starts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(1));
        #[cfg(test)]
        let worker_starts_for_task = worker_starts.clone();

        tokio::spawn(async move {
            #[cfg(test)]
            let _worker_starts = worker_starts_for_task;
            let mut in_flight = FuturesUnordered::new();
            loop {
                if in_flight.is_empty() {
                    match receiver.recv().await {
                        Some(completion) => in_flight.push(completion),
                        None => break,
                    }
                    continue;
                }
                tokio::select! {
                    completion = receiver.recv() => match completion {
                        Some(completion) => in_flight.push(completion),
                        None => {
                            while in_flight.next().await.is_some() {}
                            break;
                        }
                    },
                    _ = in_flight.next() => {}
                }
            }
        });

        Self {
            sender,
            #[cfg(test)]
            worker_starts,
        }
    }

    pub(super) fn dispatch(&self, completion: Completion) {
        match self.sender.try_send(completion) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(completion))
            | Err(mpsc::error::TrySendError::Closed(completion)) => {
                tokio::spawn(completion);
            }
        }
    }

    #[cfg(test)]
    pub(super) fn worker_starts(&self) -> usize {
        self.worker_starts
            .load(std::sync::atomic::Ordering::Acquire)
    }
}

impl MeshAwareHttpClient {
    pub(super) fn dispatch_completion(
        &self,
        completion: impl std::future::Future<Output = ()> + Send + 'static,
    ) {
        self.completion_dispatcher
            .get_or_init(CompletionDispatcher::new)
            .dispatch(Box::pin(completion));
    }

    #[cfg(test)]
    pub(crate) fn completion_worker_starts_for_test(&self) -> Option<usize> {
        self.completion_dispatcher
            .get()
            .map(CompletionDispatcher::worker_starts)
    }
}
