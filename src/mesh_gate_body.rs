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

struct GuardCell(Mutex<Option<GateGuard>>);

impl GuardCell {
    fn take(&self) -> Option<GateGuard> {
        self.0.lock().ok().and_then(|mut guard| guard.take())
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
    fn finish(&mut self) {
        self.finished = true;
        if let Some(cancel_timer) = self.cancel_timer.take() {
            let _ = cancel_timer.send(());
        }
        drop(self.guard.take());
    }
}

impl Drop for GuardedBodyState {
    fn drop(&mut self) {
        self.finish();
    }
}

pub(crate) fn guard_stream(
    body: impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static,
    gate_guard: GateGuard,
    deadline: Instant,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    let guard = Arc::new(GuardCell(Mutex::new(Some(gate_guard))));
    let (cancel_timer, timer_cancelled) = oneshot::channel();
    let timer_guard = Arc::clone(&guard);
    tokio::spawn(async move {
        tokio::select! {
            _ = time::sleep_until(time::Instant::from_std(deadline)) => {
                drop(timer_guard.take());
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
            state.finish();
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
                state.finish();
                Some((Err(error), state))
            }
            Ok(None) => {
                state.finish();
                None
            }
            Err(_) => {
                state.finish();
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
