use std::{
    io,
    sync::{Arc, Mutex},
    time::Instant,
};

use bytes::Bytes;
use futures_util::{Stream, StreamExt, stream};
use tokio::{
    sync::{OwnedRwLockReadGuard, mpsc, oneshot, watch},
    time,
};

type BodyStream = futures_util::stream::BoxStream<'static, Result<Bytes, io::Error>>;
type GateGuard = OwnedRwLockReadGuard<()>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyFinish {
    Complete,
    Error,
    Cancelled,
    Deadline,
    LeaseExpired,
}

pub(crate) type FinishCallback = Box<dyn FnOnce(BodyFinish) + Send + 'static>;

struct GuardCell {
    state: Mutex<(Option<GateGuard>, Option<FinishCallback>)>,
    cancellation: watch::Sender<Option<BodyFinish>>,
}

impl GuardCell {
    fn new(
        gate_guard: Option<GateGuard>,
        on_finish: Option<FinishCallback>,
    ) -> (Arc<Self>, watch::Receiver<Option<BodyFinish>>) {
        let (cancellation, cancellation_rx) = watch::channel(None);
        (
            Arc::new(Self {
                state: Mutex::new((gate_guard, on_finish)),
                cancellation,
            }),
            cancellation_rx,
        )
    }

    fn finish(&self, outcome: BodyFinish) {
        let Some((guard, callback)) = self
            .state
            .lock()
            .ok()
            .map(|mut state| (state.0.take(), state.1.take()))
        else {
            return;
        };
        let _ = self.cancellation.send(Some(outcome));
        drop(guard);
        if let Some(callback) = callback {
            callback(outcome);
        }
    }
}

struct GuardedBodyState {
    body: BodyStream,
    guard: Arc<GuardCell>,
    timer: Option<tokio::task::JoinHandle<()>>,
    cancellation_rx: watch::Receiver<Option<BodyFinish>>,
    deadline: Instant,
    first_byte_tx: Option<oneshot::Sender<()>>,
    lease_expiry_is_eof: bool,
    first_byte_seen: bool,
    finished: bool,
}

impl GuardedBodyState {
    fn finish(&mut self, outcome: BodyFinish) {
        self.finished = true;
        if let Some(timer) = self.timer.take() {
            timer.abort();
        }
        self.guard.finish(outcome);
    }
}

impl Drop for GuardedBodyState {
    fn drop(&mut self) {
        if !self.finished {
            self.finish(BodyFinish::Cancelled);
        }
    }
}

pub(crate) fn guard_stream(
    body: impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static,
    gate_guard: GateGuard,
    deadline: Instant,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    guard_stream_with_finish(body, gate_guard, deadline, None)
}

pub(crate) fn guard_stream_with_finish(
    body: impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static,
    gate_guard: GateGuard,
    deadline: Instant,
    on_finish: Option<FinishCallback>,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    stream_with_finish_inner(body, Some(gate_guard), deadline, on_finish, None)
}

pub(crate) fn guard_stream_with_body_lease(
    body: impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static,
    gate_guard: GateGuard,
    first_byte_deadline: Instant,
    lease: std::time::Duration,
    on_finish: Option<FinishCallback>,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    stream_with_finish_inner(
        body,
        Some(gate_guard),
        first_byte_deadline,
        on_finish,
        Some(lease),
    )
}

pub(crate) fn stream_with_finish(
    body: impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static,
    deadline: Instant,
    on_finish: Option<FinishCallback>,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    stream_with_finish_inner(body, None, deadline, on_finish, None)
}

pub(crate) fn stream_with_body_lease(
    body: impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static,
    first_byte_deadline: Instant,
    lease: std::time::Duration,
    on_finish: Option<FinishCallback>,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    stream_with_finish_inner(body, None, first_byte_deadline, on_finish, Some(lease))
}

fn stream_with_finish_inner(
    body: impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static,
    gate_guard: Option<GateGuard>,
    deadline: Instant,
    on_finish: Option<FinishCallback>,
    lease: Option<std::time::Duration>,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    let (guard, cancellation_rx) = GuardCell::new(gate_guard, on_finish);
    let body = match lease {
        Some(_) => {
            let (body_tx, body_rx) = mpsc::channel(1);
            let mut body_cancellation_rx = cancellation_rx.clone();
            tokio::spawn(async move {
                let mut body = body.boxed();
                while let Some(item) = tokio::select! {
                    _ = wait_for_cancellation(&mut body_cancellation_rx) => None,
                    item = body.next() => item,
                } {
                    let send = body_tx.send(item);
                    tokio::pin!(send);
                    tokio::select! {
                        _ = wait_for_cancellation(&mut body_cancellation_rx) => break,
                        result = &mut send => {
                            if result.is_err() {
                                break;
                            }
                        }
                    }
                }
            });
            stream::unfold(body_rx, |mut body_rx| async move {
                body_rx.recv().await.map(|item| (item, body_rx))
            })
            .boxed()
        }
        None => body.boxed(),
    };
    let timer_guard = Arc::clone(&guard);
    let (first_byte_tx, timer) = match lease {
        Some(lease) => {
            let (first_byte_tx, first_byte_rx) = oneshot::channel();
            let timer = tokio::spawn(async move {
                tokio::select! {
                    _ = time::sleep_until(time::Instant::from_std(deadline)) => {
                        timer_guard.finish(BodyFinish::Deadline);
                    }
                    result = first_byte_rx => {
                        if result.is_err() {
                            return;
                        }
                        time::sleep(lease).await;
                        timer_guard.finish(BodyFinish::LeaseExpired);
                    }
                }
            });
            (Some(first_byte_tx), timer)
        }
        None => {
            let timer = tokio::spawn(async move {
                time::sleep_until(time::Instant::from_std(deadline)).await;
                timer_guard.finish(BodyFinish::Deadline);
            });
            (None, timer)
        }
    };

    let state = GuardedBodyState {
        body,
        guard,
        timer: Some(timer),
        cancellation_rx,
        deadline,
        first_byte_tx,
        lease_expiry_is_eof: lease.is_some(),
        first_byte_seen: false,
        finished: false,
    };
    stream::unfold(state, move |mut state| async move {
        if state.finished {
            return None;
        }
        let cancellation = state.cancellation_rx.borrow().as_ref().copied();
        if let Some(outcome) = cancellation {
            return finish_after_cancellation(state, outcome);
        }
        if state.deadline <= Instant::now() {
            if state.first_byte_seen && state.lease_expiry_is_eof {
                state.finish(BodyFinish::LeaseExpired);
                return None;
            }
            state.finish(BodyFinish::Deadline);
            return Some((
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Mesh response body deadline exceeded",
                )),
                state,
            ));
        }
        enum PollOutcome {
            Cancellation(BodyFinish),
            Body(Result<Option<Result<Bytes, io::Error>>, time::error::Elapsed>),
        }
        let polled = {
            let next = state.body.next();
            tokio::pin!(next);
            tokio::select! {
                outcome = wait_for_cancellation(&mut state.cancellation_rx) => {
                    PollOutcome::Cancellation(outcome)
                }
                result = time::timeout_at(time::Instant::from_std(state.deadline), &mut next) => {
                    PollOutcome::Body(result)
                }
            }
        };
        match polled {
            PollOutcome::Cancellation(outcome) => finish_after_cancellation(state, outcome),
            PollOutcome::Body(Ok(Some(Ok(item)))) => {
                if let Some(first_byte_tx) = state.first_byte_tx.take() {
                    let _ = first_byte_tx.send(());
                    state.deadline = Instant::now()
                        .checked_add(lease.expect("first-byte lease must be configured"))
                        .unwrap_or(Instant::now());
                    state.first_byte_seen = true;
                }
                Some((Ok(item), state))
            }
            PollOutcome::Body(Ok(Some(Err(error)))) => {
                state.finish(BodyFinish::Error);
                Some((Err(error), state))
            }
            PollOutcome::Body(Ok(None)) => {
                state.finish(BodyFinish::Complete);
                None
            }
            PollOutcome::Body(Err(_)) => {
                if state.first_byte_seen && state.lease_expiry_is_eof {
                    state.finish(BodyFinish::LeaseExpired);
                    return None;
                }
                state.finish(BodyFinish::Deadline);
                Some((
                    Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Mesh response body deadline exceeded",
                    )),
                    state,
                ))
            }
        }
    })
}

async fn wait_for_cancellation(rx: &mut watch::Receiver<Option<BodyFinish>>) -> BodyFinish {
    loop {
        if let Some(outcome) = rx.borrow().as_ref().copied() {
            return outcome;
        }
        if rx.changed().await.is_err() {
            return BodyFinish::Cancelled;
        }
    }
}

fn finish_after_cancellation(
    mut state: GuardedBodyState,
    outcome: BodyFinish,
) -> Option<(Result<Bytes, io::Error>, GuardedBodyState)> {
    let emit_timeout = outcome == BodyFinish::Deadline && !state.first_byte_seen;
    state.finish(outcome);
    emit_timeout.then(|| {
        (
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Mesh response body deadline exceeded",
            )),
            state,
        )
    })
}
