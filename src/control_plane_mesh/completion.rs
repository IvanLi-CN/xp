use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc};

use futures_util::{StreamExt, stream::FuturesUnordered};
use tokio::sync::{Notify, mpsc};

use super::MeshAwareHttpClient;

type Completion = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
struct CriticalCompletion {
    key: String,
    completion: Completion,
}

const COMPLETION_QUEUE_CAPACITY: usize = 256;
const COMPLETION_ACTIVE_CAPACITY: usize = 32;

#[derive(Clone)]
pub(super) struct CompletionDispatcher {
    critical_sender: mpsc::Sender<CriticalCompletion>,
    // Saturation keeps the latest critical state per peer instead of dropping it.
    critical_pending: Arc<std::sync::Mutex<BTreeMap<String, Completion>>>,
    critical_notify: Arc<Notify>,
    #[cfg(test)]
    worker_starts: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl CompletionDispatcher {
    pub(super) fn new() -> Self {
        let (critical_sender, mut critical_receiver) =
            mpsc::channel::<CriticalCompletion>(COMPLETION_QUEUE_CAPACITY);
        let critical_pending = Arc::new(std::sync::Mutex::new(BTreeMap::new()));
        let critical_pending_for_task = critical_pending.clone();
        let critical_notify = Arc::new(Notify::new());
        let critical_notify_for_task = critical_notify.clone();
        #[cfg(test)]
        let worker_starts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(1));
        #[cfg(test)]
        let worker_starts_for_task = worker_starts.clone();

        tokio::spawn(async move {
            #[cfg(test)]
            let _worker_starts = worker_starts_for_task;
            let mut in_flight: FuturesUnordered<Completion> = FuturesUnordered::new();
            loop {
                while in_flight.len() < COMPLETION_ACTIVE_CAPACITY {
                    let mut received = false;
                    match critical_receiver.try_recv() {
                        Ok(completion) => {
                            in_flight.push(completion.completion);
                            received = true;
                        }
                        Err(mpsc::error::TryRecvError::Empty) => {}
                        Err(mpsc::error::TryRecvError::Disconnected) => {}
                    }
                    if in_flight.len() >= COMPLETION_ACTIVE_CAPACITY {
                        continue;
                    }
                    if let Some(completion) =
                        critical_pending_for_task
                            .lock()
                            .ok()
                            .and_then(|mut pending| {
                                pending.pop_first().map(|(_, completion)| completion)
                            })
                    {
                        in_flight.push(completion);
                        received = true;
                    }
                    if !received {
                        break;
                    }
                }
                if in_flight.is_empty() {
                    tokio::select! {
                        completion = critical_receiver.recv() => match completion {
                            Some(completion) => in_flight.push(completion.completion),
                            None => return,
                        },
                        _ = critical_notify_for_task.notified() => {}
                    }
                    continue;
                }
                if in_flight.len() >= COMPLETION_ACTIVE_CAPACITY {
                    let _ = in_flight.next().await;
                    continue;
                }
                tokio::select! {
                    _ = in_flight.next() => {}
                    completion = critical_receiver.recv() => match completion {
                        Some(completion) => in_flight.push(completion.completion),
                        None => return,
                    },
                    _ = critical_notify_for_task.notified() => {}
                }
            }
        });

        Self {
            critical_sender,
            critical_pending,
            critical_notify,
            #[cfg(test)]
            worker_starts,
        }
    }

    pub(super) fn dispatch_critical(
        &self,
        key: String,
        completion: impl Future<Output = ()> + Send + 'static,
    ) {
        let completion = CriticalCompletion {
            key,
            completion: Box::pin(completion),
        };
        match self.critical_sender.try_send(completion) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(completion))
            | Err(mpsc::error::TrySendError::Closed(completion)) => {
                if let Ok(mut pending) = self.critical_pending.lock() {
                    pending.insert(completion.key, completion.completion);
                }
                self.critical_notify.notify_one();
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
    pub(super) fn dispatch_critical_completion(
        &self,
        key: impl Into<String>,
        completion: impl std::future::Future<Output = ()> + Send + 'static,
    ) {
        self.completion_dispatcher
            .get_or_init(CompletionDispatcher::new)
            .dispatch_critical(key.into(), completion);
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
    async fn critical_dispatcher_bounds_active_work() {
        let dispatcher = CompletionDispatcher::new();
        let release = std::sync::Arc::new(AtomicBool::new(false));
        let active = std::sync::Arc::new(AtomicUsize::new(0));
        let max_active = std::sync::Arc::new(AtomicUsize::new(0));

        for _ in 0..(COMPLETION_QUEUE_CAPACITY + COMPLETION_ACTIVE_CAPACITY + 8) {
            let release = release.clone();
            let active = active.clone();
            let max_active = max_active.clone();
            dispatcher.dispatch_critical("mesh:peer".to_string(), async move {
                let current = active.fetch_add(1, Ordering::AcqRel) + 1;
                max_active.fetch_max(current, Ordering::AcqRel);
                while !release.load(Ordering::Acquire) {
                    tokio::task::yield_now().await;
                }
                active.fetch_sub(1, Ordering::AcqRel);
            });
        }

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if active.load(Ordering::Acquire) == COMPLETION_ACTIVE_CAPACITY {
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

    #[tokio::test]
    async fn critical_completion_survives_queue_saturation() {
        let dispatcher = CompletionDispatcher::new();
        let release = std::sync::Arc::new(AtomicBool::new(false));
        let latest_completed = std::sync::Arc::new(AtomicBool::new(false));

        for index in 0..(COMPLETION_QUEUE_CAPACITY + COMPLETION_ACTIVE_CAPACITY + 8) {
            let release = release.clone();
            let latest_completed = latest_completed.clone();
            dispatcher.dispatch_critical("mesh:peer".to_string(), async move {
                while !release.load(Ordering::Acquire) {
                    tokio::task::yield_now().await;
                }
                if index == COMPLETION_QUEUE_CAPACITY + COMPLETION_ACTIVE_CAPACITY + 7 {
                    latest_completed.store(true, Ordering::Release);
                }
            });
        }

        release.store(true, Ordering::Release);
        tokio::time::timeout(Duration::from_secs(1), async {
            while !latest_completed.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the latest critical completion must not be dropped");
    }
}
