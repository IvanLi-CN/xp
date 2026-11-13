use anyhow::Result;
use clap::Parser;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UnixListener, UnixStream},
    time::{Duration, timeout},
};
use xp_proxy_core::{
    CONTROL_PROTOCOL_VERSION, GenerationManager, ProxySnapshot, TrafficCounter,
    control::{ControlOperation, ControlRequest, ControlResponse, decode_frame, encode_frame},
    fallback::FallbackTarget,
    reality::authenticate_client_hello_with_policy,
    tls_hello::parse_client_hello,
    vless::{decode_request, is_authorized},
};

mod reality_tls;
mod xhttp_h2;

#[derive(Debug, Parser)]
#[command(name = "xp-proxy")]
struct Args {
    #[arg(long)]
    validate_snapshot: Option<std::path::PathBuf>,
    #[arg(long)]
    control_socket: Option<std::path::PathBuf>,
    #[arg(long)]
    snapshot: Option<std::path::PathBuf>,
}

#[derive(Default)]
struct ListenerRuntime {
    bound: Mutex<HashSet<std::net::SocketAddr>>,
}

impl ListenerRuntime {
    fn register(&self, bind: std::net::SocketAddr) -> bool {
        self.bound
            .lock()
            .expect("listener runtime lock poisoned")
            .insert(bind)
    }

    fn unregister(&self, bind: std::net::SocketAddr) {
        self.bound
            .lock()
            .expect("listener runtime lock poisoned")
            .remove(&bind);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if let Some(path) = args.validate_snapshot {
        let bytes = std::fs::read(path)?;
        let snapshot: ProxySnapshot = serde_json::from_slice(&bytes)?;
        snapshot.validate().map_err(anyhow::Error::msg)?;
        println!("snapshot_valid generation={}", snapshot.generation);
        return Ok(());
    }
    let socket = args
        .control_socket
        .ok_or_else(|| anyhow::anyhow!("--control-socket is required"))?;
    if socket.exists() {
        std::fs::remove_file(&socket)?;
    }
    let listener = UnixListener::bind(&socket)?;
    std::fs::set_permissions(&socket, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    let manager = GenerationManager::new();
    let traffic = Arc::new(TrafficCounter::default());
    let listener_runtime = Arc::new(ListenerRuntime::default());
    if let Some(path) = args.snapshot {
        let snapshot: ProxySnapshot = serde_json::from_slice(&std::fs::read(path)?)?;
        manager.prepare(snapshot).map_err(anyhow::Error::msg)?;
        manager.activate().map_err(anyhow::Error::msg)?;
        let active = manager.active_snapshot().expect("activated snapshot");
        start_snapshot_listeners(
            active,
            manager.clone(),
            listener_runtime.clone(),
            traffic.clone(),
        )
        .await?;
    }
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let manager = manager.clone();
                let listener_runtime = listener_runtime.clone();
                let traffic = traffic.clone();
                tokio::spawn(async move {
                    let _ = handle_control(stream, manager, listener_runtime, traffic).await;
                });
            }
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    let _ = std::fs::remove_file(socket);
    Ok(())
}

async fn handle_connection(
    stream: TcpStream,
    users: Vec<xp_proxy_core::control::UserSnapshot>,
    listener: xp_proxy_core::control::ListenerSnapshot,
    traffic: Arc<TrafficCounter>,
) -> Result<()> {
    let (size, probe) = peek_probe(&stream).await?;
    if size == 4 && &probe == b"PRI " {
        return xhttp_h2::serve(stream, users, listener.path, listener.xmux.into(), traffic).await;
    }
    if size >= 1 && probe[0] == 0x16 {
        return handle_tls_or_fallback(stream, users, listener, traffic).await;
    }
    if size >= 1 && is_http_request_byte(probe[0]) {
        return handle_raw_fallback(stream, listener, traffic).await;
    }
    handle_vless(stream, users, traffic).await
}

async fn peek_probe(stream: &TcpStream) -> Result<(usize, [u8; 4])> {
    let mut probe = [0u8; 4];
    let first = stream.peek(&mut probe).await?;
    if first == 0 || first >= 4 || probe[0] != b'P' {
        return Ok((first, probe));
    }
    let result = timeout(Duration::from_millis(100), async {
        loop {
            let size = stream.peek(&mut probe).await?;
            if size >= 4 || size == 0 {
                return Ok::<usize, std::io::Error>(size);
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    Ok((result.unwrap_or(Ok(first))?, probe))
}

fn is_http_request_byte(byte: u8) -> bool {
    matches!(byte, b'G' | b'P' | b'H' | b'D' | b'C' | b'O')
}

async fn handle_raw_fallback(
    inbound: TcpStream,
    listener: xp_proxy_core::control::ListenerSnapshot,
    traffic: Arc<TrafficCounter>,
) -> Result<()> {
    let fallback = listener
        .fallbacks
        .first()
        .ok_or_else(|| anyhow::anyhow!("HTTP request has no fallback"))?;
    let target = FallbackTarget::parse(&fallback.target).map_err(anyhow::Error::msg)?;
    if !fallback.authorities.is_empty()
        && !fallback
            .authorities
            .iter()
            .any(|authority| authority == &target.authority)
    {
        anyhow::bail!("fallback authority is not allowlisted");
    }
    let outbound = TcpStream::connect(&target.authority).await?;
    relay_tcp(inbound, outbound, traffic).await
}

async fn relay_tcp(
    inbound: TcpStream,
    outbound: TcpStream,
    traffic: Arc<TrafficCounter>,
) -> Result<()> {
    let (mut ri, mut wi) = inbound.into_split();
    let (mut ro, mut wo) = outbound.into_split();
    let uplink_counter = traffic.clone();
    let uplink = async move {
        let bytes = tokio::io::copy(&mut ri, &mut wo).await?;
        uplink_counter.add_uplink(bytes);
        Ok::<u64, std::io::Error>(bytes)
    };
    let downlink = async move {
        let bytes = tokio::io::copy(&mut ro, &mut wi).await?;
        traffic.add_downlink(bytes);
        Ok::<u64, std::io::Error>(bytes)
    };
    let _ = tokio::try_join!(uplink, downlink)?;
    Ok(())
}

async fn handle_tls_or_fallback(
    mut inbound: TcpStream,
    users: Vec<xp_proxy_core::control::UserSnapshot>,
    listener: xp_proxy_core::control::ListenerSnapshot,
    traffic: Arc<TrafficCounter>,
) -> Result<()> {
    let (hello, buffered_extra) = read_tls_client_hello(&mut inbound).await?;
    let reality = listener.reality;
    let private_key = reality.private_key_bytes().map_err(anyhow::Error::msg)?;
    let short_ids = reality.short_id_bytes().map_err(anyhow::Error::msg)?;
    let parsed = parse_client_hello(&hello);
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let authenticated = parsed.as_ref().ok().and_then(|hello| {
        authenticate_client_hello_with_policy(
            hello,
            private_key,
            &short_ids,
            &reality.server_names,
            Some(120),
            now,
        )
        .ok()
    });
    if let Some(authenticated) = authenticated {
        let server_name = parsed
            .as_ref()
            .ok()
            .and_then(|hello| hello.server_name.as_deref())
            .unwrap_or_else(|| {
                reality
                    .server_names
                    .first()
                    .map(String::as_str)
                    .unwrap_or("localhost")
            });
        let tls = reality_tls::accept(hello, buffered_extra, inbound, &authenticated, server_name)
            .await?;
        return xhttp_h2::serve(tls, users, listener.path, listener.xmux.into(), traffic).await;
    }
    let fallback = listener
        .fallbacks
        .first()
        .ok_or_else(|| anyhow::anyhow!("unauthenticated TLS has no fallback"))?;
    let target = FallbackTarget::parse(&fallback.target).map_err(anyhow::Error::msg)?;
    if !fallback.authorities.is_empty()
        && !fallback
            .authorities
            .iter()
            .any(|authority| authority == &target.authority)
    {
        anyhow::bail!("fallback authority is not allowlisted");
    }
    let mut outbound = TcpStream::connect(&target.authority).await?;
    outbound.write_all(&hello).await?;
    traffic.add_uplink(hello.len() as u64);
    if !buffered_extra.is_empty() {
        outbound.write_all(&buffered_extra).await?;
        traffic.add_uplink(buffered_extra.len() as u64);
    }
    let (mut ri, mut wi) = inbound.into_split();
    let (mut ro, mut wo) = outbound.into_split();
    let uplink_counter = traffic.clone();
    let uplink = async move {
        let bytes = tokio::io::copy(&mut ri, &mut wo).await?;
        uplink_counter.add_uplink(bytes);
        Ok::<u64, std::io::Error>(bytes)
    };
    let downlink_counter = traffic;
    let downlink = async move {
        let bytes = tokio::io::copy(&mut ro, &mut wi).await?;
        downlink_counter.add_downlink(bytes);
        Ok::<u64, std::io::Error>(bytes)
    };
    let _ = tokio::try_join!(uplink, downlink)?;
    Ok(())
}

async fn read_tls_client_hello(inbound: &mut TcpStream) -> Result<(Vec<u8>, Vec<u8>)> {
    const MAX_HELLO: usize = 64 * 1024;
    let mut bytes = Vec::with_capacity(4096);
    loop {
        if bytes.len() >= MAX_HELLO {
            anyhow::bail!("TLS ClientHello exceeds {MAX_HELLO} bytes");
        }
        let mut chunk = [0u8; 2048];
        let read = inbound.read(&mut chunk).await?;
        if read == 0 {
            anyhow::bail!("truncated TLS ClientHello");
        }
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(split) = split_tls_client_hello(&bytes) {
            return Ok(split);
        }
    }
}

fn split_tls_client_hello(bytes: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let info = parse_client_hello(bytes).ok()?;
    let consumed = info.record.len();
    Some((bytes[..consumed].to_vec(), bytes[consumed..].to_vec()))
}

async fn handle_vless(
    mut inbound: TcpStream,
    users: Vec<xp_proxy_core::control::UserSnapshot>,
    traffic: Arc<TrafficCounter>,
) -> Result<()> {
    let (header, request, consumed) = read_vless_request(&mut inbound).await?;
    let allowed = users
        .iter()
        .filter_map(|user| uuid::Uuid::parse_str(&user.uuid).ok())
        .collect::<Vec<_>>();
    if !is_authorized(&request, &allowed) {
        anyhow::bail!("unauthorized VLESS user");
    }
    let destination = format!("{}:{}", request.destination.0, request.destination.1);
    let mut outbound = TcpStream::connect(destination).await?;
    inbound.write_all(&[1, 0]).await?;
    if consumed < header.len() {
        let payload = &header[consumed..];
        outbound.write_all(payload).await?;
        traffic.add_uplink(payload.len() as u64);
    }
    let (mut ri, mut wi) = inbound.into_split();
    let (mut ro, mut wo) = outbound.into_split();
    let uplink_counter = traffic.clone();
    let downlink_counter = traffic;
    let uplink = async move {
        let bytes = tokio::io::copy(&mut ri, &mut wo).await?;
        uplink_counter.add_uplink(bytes);
        Ok::<u64, std::io::Error>(bytes)
    };
    let downlink = async move {
        let bytes = tokio::io::copy(&mut ro, &mut wi).await?;
        downlink_counter.add_downlink(bytes);
        Ok::<u64, std::io::Error>(bytes)
    };
    let _ = tokio::try_join!(uplink, downlink)?;
    Ok(())
}

async fn read_vless_request(
    inbound: &mut TcpStream,
) -> Result<(Vec<u8>, xp_proxy_core::vless::VlessRequest, usize)> {
    const MAX_HEADER: usize = 8192;
    let mut header = Vec::with_capacity(256);
    loop {
        if header.len() == MAX_HEADER {
            anyhow::bail!("VLESS request header exceeds {MAX_HEADER} bytes");
        }
        let mut chunk = [0u8; 512];
        let read = inbound.read(&mut chunk).await?;
        if read == 0 {
            anyhow::bail!("empty or truncated VLESS request");
        }
        header.extend_from_slice(&chunk[..read]);
        match decode_request(&header) {
            Ok((request, consumed)) => return Ok((header, request, consumed)),
            Err(xp_proxy_core::vless::VlessRequestError::Truncated) => continue,
            Err(error) => return Err(anyhow::anyhow!(error)),
        }
    }
}

async fn start_snapshot_listeners(
    snapshot: ProxySnapshot,
    manager: GenerationManager,
    runtime: Arc<ListenerRuntime>,
    traffic: Arc<TrafficCounter>,
) -> Result<()> {
    for listener in snapshot.listeners {
        let bind = listener.bind.parse::<std::net::SocketAddr>()?;
        if !runtime.register(bind) {
            continue;
        }
        let tcp = match TcpListener::bind(bind).await {
            Ok(tcp) => tcp,
            Err(error) => {
                runtime.unregister(bind);
                return Err(error.into());
            }
        };
        let manager = manager.clone();
        let runtime = runtime.clone();
        let traffic = traffic.clone();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = tcp.accept().await else {
                    runtime.unregister(bind);
                    break;
                };
                let Some(active) = manager.active_snapshot() else {
                    continue;
                };
                let Some(listener) = active
                    .listeners
                    .iter()
                    .find(|candidate| candidate.bind == bind.to_string())
                    .cloned()
                else {
                    continue;
                };
                let users = active.users;
                let traffic = traffic.clone();
                tokio::spawn(async move {
                    if let Err(error) = handle_connection(stream, users, listener, traffic).await {
                        eprintln!("proxy connection failed: {error:#}");
                    }
                });
            }
        });
    }
    Ok(())
}

async fn handle_control(
    mut stream: UnixStream,
    manager: GenerationManager,
    listener_runtime: Arc<ListenerRuntime>,
    traffic: Arc<TrafficCounter>,
) -> Result<()> {
    loop {
        let mut header = [0u8; 4];
        if stream.read_exact(&mut header).await.is_err() {
            return Ok(());
        }
        let size = u32::from_be_bytes(header) as usize;
        if size > 16 * 1024 * 1024 {
            anyhow::bail!("control frame too large");
        }
        let mut body = vec![0u8; size];
        stream.read_exact(&mut body).await?;
        let mut frame = header.to_vec();
        frame.extend_from_slice(&body);
        let request: ControlRequest = decode_frame(&frame).map_err(anyhow::Error::msg)?;
        if request.protocol_version != CONTROL_PROTOCOL_VERSION {
            anyhow::bail!("unsupported control protocol version");
        }
        let (_, active) = manager.state();
        let result = match request.operation {
            ControlOperation::Prepare { snapshot } => manager
                .prepare(snapshot)
                .map(|_| manager.state().1.unwrap_or(request.generation)),
            ControlOperation::Activate => {
                let snapshot = manager
                    .prepared_snapshot()
                    .ok_or_else(|| anyhow::anyhow!("no prepared generation"))?;
                start_snapshot_listeners(
                    snapshot,
                    manager.clone(),
                    listener_runtime.clone(),
                    traffic.clone(),
                )
                .await?;
                manager.activate()
            }
            ControlOperation::Drain => Ok(manager.drain().or(active).unwrap_or(request.generation)),
            ControlOperation::Stop => Ok(manager.stop().or(active).unwrap_or(request.generation)),
            ControlOperation::Status => Ok(active.unwrap_or(request.generation)),
        };
        let error = result.as_ref().err().cloned();
        let (state, generation) = manager.state();
        let response = ControlResponse {
            protocol_version: request.protocol_version,
            request_id: request.request_id,
            generation: generation.or(result.ok()).unwrap_or(request.generation),
            state,
            error,
            metrics: Some(traffic.snapshot()),
        };
        stream.write_all(&encode_frame(&response)?).await?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use x25519_dalek::{PublicKey, StaticSecret};
    use xp_proxy_core::reality::{RealityClientHelloConfig, build_client_hello};

    #[test]
    fn split_client_hello_preserves_buffered_tail() {
        let server_private = [9u8; 32];
        let config = RealityClientHelloConfig {
            server_name: "origin.example".into(),
            server_public_key: PublicKey::from(&StaticSecret::from(server_private)).to_bytes(),
            short_id: vec![1, 2, 3, 4, 5, 6, 7, 8],
            unix_seconds: 1_700_000_000,
            version: *b"1.8\0",
            alpn: vec!["h2".into()],
        };
        let (hello, _) = build_client_hello(&config, [7u8; 32], [3u8; 32], [4u8; 16]).unwrap();
        let tail = b"buffered application bytes";
        let mut input = hello.clone();
        input.extend_from_slice(tail);
        let (parsed, buffered) = split_tls_client_hello(&input).unwrap();
        assert_eq!(parsed, hello);
        assert_eq!(buffered, tail);
    }

    #[test]
    fn recognizes_http_methods_without_treating_vless_as_fallback() {
        assert!(is_http_request_byte(b'G'));
        assert!(is_http_request_byte(b'P'));
        assert!(is_http_request_byte(b'C'));
        assert!(!is_http_request_byte(1));
        assert!(!is_http_request_byte(b'Z'));
    }
}
