use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use futures_util::{StreamExt, stream::FuturesUnordered};
use tokio::sync::{Notify, mpsc};

use super::MeshAwareHttpClient;

type Completion = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
struct CriticalCompletion {
    key: String,
    operation_id: Option<u64>,
    completion: Completion,
}

const COMPLETION_QUEUE_CAPACITY: usize = 256;
const COMPLETION_ACTIVE_CAPACITY: usize = 32;

#[derive(Clone)]
pub(super) struct CompletionDispatcher {
    critical_sender: mpsc::Sender<CriticalCompletion>,
    // Saturation keeps the latest critical state per peer instead of dropping it.
    critical_pending: Arc<std::sync::Mutex<BTreeMap<String, CriticalCompletion>>>,
    critical_pending_ready: Arc<AtomicBool>,
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
        let critical_pending_ready = Arc::new(AtomicBool::new(false));
        let critical_pending_ready_for_task = critical_pending_ready.clone();
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
            let mut prefer_pending = false;
            loop {
                while in_flight.len() < COMPLETION_ACTIVE_CAPACITY {
                    let mut received = false;
                    if prefer_pending && critical_pending_ready_for_task.load(Ordering::Acquire) {
                        if let Some(completion) = pop_pending(
                            &critical_pending_for_task,
                            &critical_pending_ready_for_task,
                        ) {
                            in_flight.push(completion);
                            received = true;
                        }
                        prefer_pending = false;
                    }
                    if in_flight.len() >= COMPLETION_ACTIVE_CAPACITY {
                        continue;
                    }
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
                    if critical_pending_ready_for_task.load(Ordering::Acquire)
                        && let Some(completion) = pop_pending(
                            &critical_pending_for_task,
                            &critical_pending_ready_for_task,
                        )
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
                    prefer_pending = true;
                    continue;
                }
                tokio::select! {
                    _ = in_flight.next() => {
                        prefer_pending = true;
                    }
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
            critical_pending_ready,
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
        self.dispatch(key, None, completion);
    }

    pub(super) fn dispatch_ordered_critical(
        &self,
        key: String,
        operation_id: u64,
        completion: impl Future<Output = ()> + Send + 'static,
    ) {
        self.dispatch(key, Some(operation_id), completion);
    }

    fn dispatch(
        &self,
        key: String,
        operation_id: Option<u64>,
        completion: impl Future<Output = ()> + Send + 'static,
    ) {
        let completion = CriticalCompletion {
            key,
            operation_id,
            completion: Box::pin(completion),
        };
        match self.critical_sender.try_send(completion) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(completion))
            | Err(mpsc::error::TrySendError::Closed(completion)) => {
                if let Ok(mut pending) = self.critical_pending.lock() {
                    if let Some(previous) = pending.get(&completion.key)
                        && let Some((previous_id, incoming_id)) =
                            previous.operation_id.zip(completion.operation_id)
                        && previous_id > incoming_id
                    {
                        return;
                    }
                    pending.insert(completion.key.clone(), completion);
                    self.critical_pending_ready.store(true, Ordering::Release);
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

fn pop_pending(
    pending: &std::sync::Mutex<BTreeMap<String, CriticalCompletion>>,
    pending_ready: &AtomicBool,
) -> Option<Completion> {
    let Ok(mut pending) = pending.lock() else {
        return None;
    };
    let completion = pending
        .pop_first()
        .map(|(_, completion)| completion.completion);
    if pending.is_empty() {
        pending_ready.store(false, Ordering::Release);
    }
    completion
}

impl MeshAwareHttpClient {
    pub(super) fn dispatch_ordered_critical_completion(
        &self,
        key: String,
        operation_id: u64,
        completion: impl Future<Output = ()> + Send + 'static,
    ) {
        self.completion_dispatcher
            .get_or_init(CompletionDispatcher::new)
            .dispatch_ordered_critical(key, operation_id, completion);
    }

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

    #[tokio::test]
    async fn pending_and_queued_completions_share_the_active_bound() {
        let dispatcher = CompletionDispatcher::new();
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();

        for index in 0..(COMPLETION_ACTIVE_CAPACITY + COMPLETION_QUEUE_CAPACITY + 2) {
            let release = release.clone();
            let active = active.clone();
            let max_active = max_active.clone();
            let started_tx = started_tx.clone();
            dispatcher.dispatch_critical(format!("mesh:peer-{index}"), async move {
                let count = active.fetch_add(1, Ordering::AcqRel) + 1;
                max_active.fetch_max(count, Ordering::AcqRel);
                let _ = started_tx.send(());
                release
                    .acquire()
                    .await
                    .expect("test release semaphore")
                    .forget();
                active.fetch_sub(1, Ordering::AcqRel);
            });
        }
        for _ in 0..COMPLETION_ACTIVE_CAPACITY {
            tokio::time::timeout(Duration::from_secs(1), started_rx.recv())
                .await
                .expect("the active window should start")
                .expect("completion start signal");
        }
        release.add_permits(1);
        tokio::time::timeout(Duration::from_secs(1), started_rx.recv())
            .await
            .expect("one free slot should admit one completion")
            .expect("completion start signal");
        let _ = tokio::time::timeout(Duration::from_millis(50), started_rx.recv()).await;
        let maximum = max_active.load(Ordering::Acquire);
        release.add_permits(COMPLETION_ACTIVE_CAPACITY + COMPLETION_QUEUE_CAPACITY + 2);
        assert_eq!(maximum, COMPLETION_ACTIVE_CAPACITY);
    }

    #[tokio::test]
    async fn pending_completion_is_serviced_while_the_channel_stays_replenished() {
        let dispatcher = CompletionDispatcher::new();
        let release = std::sync::Arc::new(AtomicBool::new(false));
        let active = std::sync::Arc::new(AtomicUsize::new(0));

        for index in 0..COMPLETION_ACTIVE_CAPACITY {
            let release = release.clone();
            let active = active.clone();
            dispatcher.dispatch_critical(format!("active-{index}"), async move {
                active.fetch_add(1, Ordering::AcqRel);
                while !release.load(Ordering::Acquire) {
                    tokio::task::yield_now().await;
                }
                active.fetch_sub(1, Ordering::AcqRel);
            });
        }
        tokio::time::timeout(Duration::from_secs(1), async {
            while active.load(Ordering::Acquire) != COMPLETION_ACTIVE_CAPACITY {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the dispatcher should fill its active window");

        let pending_completed = std::sync::Arc::new(AtomicBool::new(false));
        let pending_completed_for_task = pending_completed.clone();
        dispatcher
            .critical_pending
            .lock()
            .expect("pending completion lock")
            .insert(
                "pending-key".to_string(),
                CriticalCompletion {
                    key: "pending-key".to_string(),
                    operation_id: None,
                    completion: Box::pin(async move {
                        pending_completed_for_task.store(true, Ordering::Release);
                    }),
                },
            );
        dispatcher
            .critical_pending_ready
            .store(true, Ordering::Release);
        dispatcher.critical_notify.notify_one();

        let producer_dispatcher = dispatcher.clone();
        let pending_completed_for_producer = pending_completed.clone();
        let producer = tokio::spawn(async move {
            let mut index = 0usize;
            while !pending_completed_for_producer.load(Ordering::Acquire) {
                producer_dispatcher.dispatch_critical(format!("replenished-{index}"), async {
                    tokio::task::yield_now().await
                });
                index = index.wrapping_add(1);
                tokio::task::yield_now().await;
            }
        });
        release.store(true, Ordering::Release);

        tokio::time::timeout(Duration::from_secs(1), async {
            while !pending_completed.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("a continuously replenished channel must not starve pending state");
        producer.abort();
    }
}
