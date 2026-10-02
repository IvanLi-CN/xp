use std::{future::Future, pin::Pin};

use futures_util::{StreamExt, stream::FuturesUnordered};
use tokio::sync::{Semaphore, mpsc};

use super::MeshAwareHttpClient;

type Completion = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

const COMPLETION_QUEUE_CAPACITY: usize = 256;
const COMPLETION_ACTIVE_CAPACITY: usize = 32;

#[derive(Clone)]
pub(super) struct CompletionDispatcher {
    sender: mpsc::Sender<Completion>,
    #[cfg(test)]
    worker_starts: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    #[cfg(test)]
    dropped_completions: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl CompletionDispatcher {
    pub(super) fn new() -> Self {
        let (sender, mut receiver) = mpsc::channel(COMPLETION_QUEUE_CAPACITY);
        let active = std::sync::Arc::new(Semaphore::new(COMPLETION_ACTIVE_CAPACITY));
        #[cfg(test)]
        let worker_starts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(1));
        #[cfg(test)]
        let worker_starts_for_task = worker_starts.clone();
        #[cfg(test)]
        let dropped_completions = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

        tokio::spawn(async move {
            #[cfg(test)]
            let _worker_starts = worker_starts_for_task;
            let mut in_flight: FuturesUnordered<Completion> = FuturesUnordered::new();
            loop {
                while active.available_permits() > 0 {
                    let permit = active
                        .clone()
                        .try_acquire_owned()
                        .expect("completion permit must be available");
                    match receiver.try_recv() {
                        Ok(completion) => in_flight.push(with_permit(completion, permit)),
                        Err(mpsc::error::TryRecvError::Empty) => {
                            drop(permit);
                            break;
                        }
                        Err(mpsc::error::TryRecvError::Disconnected) => {
                            drop(permit);
                            while in_flight.next().await.is_some() {}
                            return;
                        }
                    }
                }
                if in_flight.is_empty() {
                    let Some(completion) = receiver.recv().await else {
                        break;
                    };
                    let permit = active
                        .clone()
                        .acquire_owned()
                        .await
                        .expect("completion worker semaphore must remain open");
                    in_flight.push(with_permit(completion, permit));
                    continue;
                }
                if active.available_permits() == 0 {
                    let _ = in_flight.next().await;
                    continue;
                }
                tokio::select! {
                    _ = in_flight.next() => {}
                    completion = receiver.recv() => match completion {
                        Some(completion) => {
                            let permit = active
                                .clone()
                                .try_acquire_owned()
                                .expect("completion permit must be available");
                            in_flight.push(with_permit(completion, permit));
                        }
                        None => {
                            while in_flight.next().await.is_some() {}
                            break;
                        }
                    },
                }
            }
        });

        Self {
            sender,
            #[cfg(test)]
            worker_starts,
            #[cfg(test)]
            dropped_completions,
        }
    }

    pub(super) fn dispatch(&self, completion: Completion) {
        match self.sender.try_send(completion) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) | Err(mpsc::error::TrySendError::Closed(_)) => {
                #[cfg(test)]
                self.dropped_completions
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                tracing::debug!("Mesh completion dispatcher queue is full or closed");
            }
        }
    }

    #[cfg(test)]
    pub(super) fn worker_starts(&self) -> usize {
        self.worker_starts
            .load(std::sync::atomic::Ordering::Acquire)
    }

    #[cfg(test)]
    pub(super) fn dropped_completions(&self) -> usize {
        self.dropped_completions
            .load(std::sync::atomic::Ordering::Acquire)
    }
}

fn with_permit(completion: Completion, permit: tokio::sync::OwnedSemaphorePermit) -> Completion {
    Box::pin(async move {
        completion.await;
        drop(permit);
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn completion_dispatcher_bounds_active_work() {
        let dispatcher = CompletionDispatcher::new();
        let release = std::sync::Arc::new(AtomicBool::new(false));
        let active = std::sync::Arc::new(AtomicUsize::new(0));
        let max_active = std::sync::Arc::new(AtomicUsize::new(0));

        for _ in 0..(COMPLETION_QUEUE_CAPACITY + COMPLETION_ACTIVE_CAPACITY + 8) {
            let release = release.clone();
            let active = active.clone();
            let max_active = max_active.clone();
            dispatcher.dispatch(Box::pin(async move {
                let current = active.fetch_add(1, Ordering::AcqRel) + 1;
                max_active.fetch_max(current, Ordering::AcqRel);
                while !release.load(Ordering::Acquire) {
                    tokio::task::yield_now().await;
                }
                active.fetch_sub(1, Ordering::AcqRel);
            }));
        }

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if active.load(Ordering::Acquire) == COMPLETION_ACTIVE_CAPACITY
                    && dispatcher.dropped_completions() > 0
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("completion dispatcher should expose its active bound");

        assert_eq!(
            max_active.load(Ordering::Acquire),
            COMPLETION_ACTIVE_CAPACITY
        );
        release.store(true, Ordering::Release);
    }
}
