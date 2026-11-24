//! Outbound TCP dialing shared by every xp-proxy data path.
//!
//! A plain `TcpStream::connect((host, port))` walks the resolved addresses in order and
//! waits for each one to complete. When the first address is an IPv6 address with a
//! black-holed route, the connect hangs until the kernel gives up, so the request never
//! reaches a working IPv4 address and the client sees a timeout.
//!
//! `connect_tcp`/`connect_authority` start the next candidate a short delay after the
//! previous one and return the first connection that succeeds, so a hanging family can no
//! longer stall the request.

use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::{TcpStream, ToSocketAddrs, lookup_host};

/// Delay before dialing the next resolved address while the previous attempt is pending.
const HAPPY_EYEBALLS_DELAY: Duration = Duration::from_millis(250);

/// Dials `host:port`, racing the resolved addresses instead of trying them strictly in order.
pub(crate) async fn connect_tcp(host: &str, port: u16) -> io::Result<TcpStream> {
    connect_resolved((host, port)).await
}

/// Dials an already formatted authority such as `127.0.0.1:39043` or `[::1]:39043`.
pub(crate) async fn connect_authority(authority: &str) -> io::Result<TcpStream> {
    connect_resolved(authority).await
}

async fn connect_resolved(addrs: impl ToSocketAddrs) -> io::Result<TcpStream> {
    let candidates: Vec<SocketAddr> = lookup_host(addrs).await?.collect();
    let connect = |addr| async move { TcpStream::connect(addr).await };
    match candidates.as_slice() {
        [] => Err(io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            "no resolved address to dial",
        )),
        // No choice to race: keep the direct connect (and its zero extra scheduling cost).
        [addr] => connect(*addr).await,
        _ => first_connection(candidates, HAPPY_EYEBALLS_DELAY, connect).await,
    }
}

/// Returns the first successful connection, starting one more candidate every `delay` while
/// the current attempts are still pending. A failed attempt immediately starts the next one.
async fn first_connection<T, F, Fut>(
    candidates: Vec<SocketAddr>,
    delay: Duration,
    connect: F,
) -> io::Result<T>
where
    T: Send + 'static,
    F: Fn(SocketAddr) -> Fut + Send + 'static,
    Fut: Future<Output = io::Result<T>> + Send + 'static,
{
    let mut pending: VecDeque<SocketAddr> = candidates.into();
    let mut tasks = tokio::task::JoinSet::new();
    let mut last_error = None;

    if let Some(addr) = pending.pop_front() {
        tasks.spawn(connect(addr));
    }

    while !tasks.is_empty() {
        tokio::select! {
            biased;
            joined = tasks.join_next() => match joined {
                Some(Ok(Ok(connection))) => {
                    tasks.abort_all();
                    return Ok(connection);
                }
                Some(Ok(Err(error))) => {
                    last_error = Some(error);
                    if let Some(addr) = pending.pop_front() {
                        tasks.spawn(connect(addr));
                    }
                }
                Some(Err(error)) => {
                    last_error = Some(io::Error::other(error));
                    if let Some(addr) = pending.pop_front() {
                        tasks.spawn(connect(addr));
                    }
                }
                None => break,
            },
            _ = tokio::time::sleep(delay), if !pending.is_empty() => {
                if let Some(addr) = pending.pop_front() {
                    tasks.spawn(connect(addr));
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            "no resolved address to dial",
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::net::TcpListener;

    fn v6() -> SocketAddr {
        "[2001:db8::1]:9".parse().unwrap()
    }

    fn v4() -> SocketAddr {
        "192.0.2.1:9".parse().unwrap()
    }

    #[tokio::test]
    async fn hanging_first_address_falls_back_to_the_next() {
        let started = Arc::new(AtomicUsize::new(0));
        let counter = started.clone();
        let target = v4();
        let result = first_connection(vec![v6(), v4()], Duration::from_millis(10), move |addr| {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                if addr == target {
                    Ok(addr)
                } else {
                    std::future::pending::<()>().await;
                    unreachable!()
                }
            }
        })
        .await
        .expect("the reachable address must win while the first one hangs");
        assert_eq!(result, v4());
        assert_eq!(started.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn an_immediate_success_does_not_start_the_next_candidate() {
        let started = Arc::new(AtomicUsize::new(0));
        let counter = started.clone();
        let result = first_connection(vec![v4(), v6()], Duration::from_millis(50), move |addr| {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(addr)
            }
        })
        .await
        .unwrap();
        assert_eq!(result, v4());
        assert_eq!(started.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_failed_attempt_starts_the_next_one_without_waiting_for_the_stagger() {
        let result = first_connection(
            vec![v6(), v4()],
            Duration::from_secs(5),
            |addr| async move {
                if addr == v4() {
                    Ok(addr)
                } else {
                    Err(io::Error::new(io::ErrorKind::ConnectionRefused, "refused"))
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(result, v4());
    }

    #[tokio::test]
    async fn every_failure_returns_the_last_error() {
        let error = first_connection(vec![v6(), v4()], Duration::from_millis(5), |_| async {
            Err::<SocketAddr, io::Error>(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                "refused",
            ))
        })
        .await
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::ConnectionRefused);
    }

    #[tokio::test]
    async fn no_candidates_is_an_address_error() {
        let error = first_connection(Vec::new(), Duration::from_millis(5), |addr| async move {
            Ok(addr)
        })
        .await
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AddrNotAvailable);
    }

    #[tokio::test]
    async fn connect_tcp_and_authority_reach_a_real_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(connect_tcp("127.0.0.1", port).await.is_ok());
        let authority = format!("127.0.0.1:{port}");
        assert!(connect_authority(&authority).await.is_ok());
    }
}
