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
    cancel_timer: Option<oneshot::Sender<()>>,
    deadline: Instant,
    finished: bool,
}

impl GuardedBodyState {
    fn finish(&mut self, outcome: BodyFinish) {
        self.finished = true;
        if let Some(cancel_timer) = self.cancel_timer.take() {
            let _ = cancel_timer.send(());
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
    stream_with_finish_inner(body, Some(gate_guard), deadline, on_finish)
}

pub(crate) fn stream_with_finish(
    body: impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static,
    deadline: Instant,
    on_finish: Option<FinishCallback>,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    stream_with_finish_inner(body, None, deadline, on_finish)
}

fn stream_with_finish_inner(
    body: impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static,
    gate_guard: Option<GateGuard>,
    deadline: Instant,
    on_finish: Option<FinishCallback>,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    let guard = Arc::new(GuardCell(Mutex::new((gate_guard, on_finish))));
    let (cancel_timer, timer_cancelled) = oneshot::channel();
    let timer_guard = Arc::clone(&guard);
    tokio::spawn(async move {
        tokio::select! {
            _ = time::sleep_until(time::Instant::from_std(deadline)) => {
                timer_guard.finish(BodyFinish::Deadline);
            }
            _ = timer_cancelled => {}
        }
    });

    let state = GuardedBodyState {
        body: body.boxed(),
        guard,
        cancel_timer: Some(cancel_timer),
        deadline,
        finished: false,
    };
    stream::unfold(state, |mut state| async move {
        if state.finished {
            return None;
        }
        if state.deadline <= Instant::now() {
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
            Ok(Some(Ok(item))) => Some((Ok(item), state)),
            Ok(Some(Err(error))) => {
                state.finish(BodyFinish::Error);
                Some((Err(error), state))
            }
            Ok(None) => {
                state.finish(BodyFinish::Complete);
                None
            }
            Err(_) => {
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
