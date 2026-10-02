use std::{
    io,
    sync::{Arc, Mutex},
    time::Instant,
};

use bytes::Bytes;
use futures_util::{Stream, StreamExt, stream};
use tokio::{
    sync::{OwnedRwLockReadGuard, oneshot},
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

struct GuardCell(Mutex<(Option<GateGuard>, Option<FinishCallback>)>);

impl GuardCell {
    fn finish(&self, outcome: BodyFinish) {
        let Some((guard, callback)) = self
            .0
            .lock()
            .ok()
            .map(|mut state| (state.0.take(), state.1.take()))
        else {
            return;
        };
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
    let guard = Arc::new(GuardCell(Mutex::new((gate_guard, on_finish))));
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
        body: body.boxed(),
        guard,
        timer: Some(timer),
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
        match time::timeout_at(time::Instant::from_std(state.deadline), state.body.next()).await {
            Ok(Some(Ok(item))) => {
                if let Some(first_byte_tx) = state.first_byte_tx.take() {
                    let _ = first_byte_tx.send(());
                    state.deadline = Instant::now()
                        .checked_add(lease.expect("first-byte lease must be configured"))
                        .unwrap_or(Instant::now());
                    state.first_byte_seen = true;
                }
                Some((Ok(item), state))
            }
            Ok(Some(Err(error))) => {
                state.finish(BodyFinish::Error);
                Some((Err(error), state))
            }
            Ok(None) => {
                state.finish(BodyFinish::Complete);
                None
            }
            Err(_) => {
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
