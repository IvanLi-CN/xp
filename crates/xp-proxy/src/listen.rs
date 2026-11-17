//! TCP wildcard semantics shared by snapshot and standalone outbound listeners.
use socket2::{Domain, Protocol, Socket, Type};
use std::{io, net::SocketAddr};
use tokio::net::TcpListener;

/// XP's IPv4 unspecified address denotes an automatic, dual-stack wildcard.
/// Explicit addresses retain their family and interface scope.
pub fn bind(address: SocketAddr) -> io::Result<TcpListener> {
    let automatic = address.is_ipv4() && address.ip().is_unspecified();
    if automatic {
        let dual = SocketAddr::from(([0u16; 8], address.port()));
        match bind_socket(dual, false) {
            Ok(listener) => {
                eprintln!(
                    "listener_automatic configured={address} local={}",
                    listener.local_addr()?
                );
                return Ok(listener);
            }
            Err(error) if ipv6_unavailable(&error) => {
                eprintln!("listener_ipv6_unavailable error={error}; using IPv4 wildcard");
            }
            Err(error) => return Err(error),
        }
    }
    bind_socket(address, address.is_ipv6())
}

fn bind_socket(address: SocketAddr, only_v6: bool) -> io::Result<TcpListener> {
    let socket = Socket::new(
        Domain::for_address(address),
        Type::STREAM,
        Some(Protocol::TCP),
    )?;
    socket.set_reuse_address(true)?;
    if address.is_ipv6() {
        socket.set_only_v6(only_v6)?;
    }
    socket.set_nonblocking(true)?;
    socket.bind(&address.into())?;
    socket.listen(1024)?;
    let listener = TcpListener::from_std(socket.into())?;
    eprintln!(
        "listener_bound address={address} local={} dual_stack={}",
        listener.local_addr()?,
        address.is_ipv6() && !only_v6
    );
    Ok(listener)
}

fn ipv6_unavailable(error: &io::Error) -> bool {
    // Deliberately exclude EINVAL, EACCES, EADDRINUSE and resource failures.
    // On Linux, disabled IPv6 reports EAFNOSUPPORT or EADDRNOTAVAIL.
    matches!(error.raw_os_error(), Some(code) if [
        libc::EAFNOSUPPORT,
        libc::EPROTONOSUPPORT,
        libc::ENOPROTOOPT,
        libc::EADDRNOTAVAIL,
    ].contains(&code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn automatic_wildcard_accepts_both_families_and_releases_port() {
        let listener = bind("0.0.0.0:0".parse().unwrap()).unwrap();
        let address = listener.local_addr().unwrap();
        assert!(address.is_ipv6(), "this test requires IPv6 support");
        for host in ["127.0.0.1", "::1"] {
            let client = tokio::net::TcpStream::connect((host, address.port()))
                .await
                .unwrap();
            let (server, peer) = listener.accept().await.unwrap();
            assert_eq!(server.local_addr().unwrap().port(), address.port());
            assert_eq!(peer.ip().to_canonical().is_ipv4(), host == "127.0.0.1");
            drop((client, server));
        }
        assert_eq!(bind(address).unwrap_err().kind(), io::ErrorKind::AddrInUse);
        drop(listener);
        bind(address).unwrap();
    }

    #[tokio::test]
    async fn explicit_loopbacks_keep_their_address_family() {
        for host in ["127.0.0.1:0", "[::1]:0"] {
            let configured: SocketAddr = host.parse().unwrap();
            let listener = bind(configured).unwrap();
            assert_eq!(listener.local_addr().unwrap().ip(), configured.ip());
        }
    }

    #[test]
    fn only_capability_errors_allow_ipv4_fallback() {
        for code in [
            libc::EACCES,
            libc::EADDRINUSE,
            libc::EMFILE,
            libc::ENOMEM,
            libc::EINVAL,
        ] {
            assert!(!ipv6_unavailable(&io::Error::from_raw_os_error(code)));
        }
        assert!(ipv6_unavailable(&io::Error::from_raw_os_error(
            libc::EAFNOSUPPORT
        )));
    }
}
