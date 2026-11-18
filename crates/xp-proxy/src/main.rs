use anyhow::{Context, Result};
use clap::Parser;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(test)]
use tokio::net::TcpListener;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UnixListener, UnixStream},
    sync::Notify,
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

#[cfg(test)]
mod ipv6_tests;
mod listen;
mod outbound;
#[cfg(test)]
mod outbound_tests;
mod reality_stream;
mod reality_tls;
mod xhttp_h2;

#[derive(Debug, Parser)]
#[command(name = "xp-proxy")]
struct Args {
    #[arg(long)]
    validate_snapshot: Option<std::path::PathBuf>,
    #[arg(long)]
    control_socket: Option<std::path::PathBuf>,
    #[arg(
        long,
        default_value = "/var/lib/xp-proxy/last-successful-snapshot.json"
    )]
    state_file: std::path::PathBuf,
    #[arg(long)]
    snapshot: Option<std::path::PathBuf>,
    /// Runs the explicit Rust outbound client instead of the control-plane listener.
    #[arg(long)]
    outbound_config: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
struct OutboundConfig {
    listen: std::net::SocketAddr,
    remote: std::net::SocketAddr,
    server_name: String,
    server_public_key: String,
    short_id: String,
    private_key: String,
    path: String,
    uuid: uuid::Uuid,
    destination_host: String,
    destination_port: u16,
    #[serde(default)]
    xmux: xp_proxy_core::XmuxSettings,
}

#[derive(Default)]
struct ListenerRuntime {
    bound: Mutex<HashMap<std::net::SocketAddr, Arc<Notify>>>,
    transition: tokio::sync::Mutex<()>,
}

impl ListenerRuntime {
    fn register(&self, bind: std::net::SocketAddr) -> bool {
        let mut bound = self.bound.lock().expect("listener runtime lock poisoned");
        if bound.contains_key(&bind) {
            return false;
        }
        bound.insert(bind, Arc::new(Notify::new()));
        true
    }

    fn stop_token(&self, bind: std::net::SocketAddr) -> Option<Arc<Notify>> {
        self.bound
            .lock()
            .expect("listener runtime lock poisoned")
            .remove(&bind)
    }

    async fn stop_all(&self) -> bool {
        let tokens = self
            .bound
            .lock()
            .expect("listener runtime lock poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for token in tokens {
            // Retain a permit if a newly spawned accept task has not polled yet.
            token.notify_one();
        }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if self
                .bound
                .lock()
                .expect("listener runtime lock poisoned")
                .is_empty()
            {
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::task::yield_now().await;
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if let Some(path) = args.outbound_config {
        let config: OutboundConfig = serde_json::from_slice(&std::fs::read(path)?)?;
        return run_outbound(config).await;
    }
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
    std::fs::set_permissions(&socket, std::os::unix::fs::PermissionsExt::from_mode(0o660))?;
    let manager =
        GenerationManager::new_with_persistence(&args.state_file).map_err(anyhow::Error::msg)?;
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
                    if let Err(error) =
                        handle_control(stream, manager, listener_runtime, traffic).await
                    {
                        eprintln!("xp-proxy control request failed: {error}");
                    }
                });
            }
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    let _ = std::fs::remove_file(socket);
    Ok(())
}

async fn run_outbound(config: OutboundConfig) -> Result<()> {
    let listener = listen::bind(config.listen)?;
    let pool = Arc::new(outbound::Pool::new(config)?);
    loop {
        let (local, _) = listener.accept().await?;
        local.set_nodelay(true)?;
        let pool = pool.clone();
        tokio::spawn(async move {
            if let Err(error) = pool.relay(local).await {
                eprintln!("native outbound connection failed: {error}");
            }
        });
    }
}

async fn handle_connection(
    stream: TcpStream,
    users: Vec<xp_proxy_core::control::UserSnapshot>,
    listener: xp_proxy_core::control::ListenerSnapshot,
    traffic: Arc<TrafficCounter>,
) -> Result<()> {
    stream.set_nodelay(true)?;
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
    let mut outbound = if let Some(target) = listener.reality_tls_target {
        if parsed.is_err() {
            anyhow::bail!("invalid TLS ClientHello for managed REALITY fallback");
        }
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            TcpStream::connect(target),
        )
        .await
        .context("managed REALITY TLS target dial timed out")?
        .context("managed REALITY TLS target dial failed")?
    } else {
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
        TcpStream::connect(&target.authority).await?
    };
    outbound.write_all(&hello).await?;
    traffic.add_uplink(hello.len() as u64);
    if !buffered_extra.is_empty() {
        outbound.write_all(&buffered_extra).await?;
        traffic.add_uplink(buffered_extra.len() as u64);
    }
    if listener.reality_tls_target.is_some() {
        let (uplink, downlink) = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await?;
        traffic.add_uplink(uplink);
        traffic.add_downlink(downlink);
        return Ok(());
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
    let mut outbound =
        TcpStream::connect((request.destination.0.as_str(), request.destination.1)).await?;
    inbound.write_all(&[0, 0]).await?;
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
    // Complete every bind before exposing any part of the new generation.
    let mut staged = Vec::new();
    for listener in snapshot.listeners {
        let bind = listener.bind.parse::<std::net::SocketAddr>()?;
        let tcp = listen::bind(bind)?;
        staged.push((bind, tcp));
    }
    for (bind, tcp) in staged {
        if !runtime.register(bind) {
            anyhow::bail!("listener already registered: {bind}");
        }
        let stop = runtime
            .bound
            .lock()
            .expect("listener runtime lock poisoned")
            .get(&bind)
            .cloned()
            .expect("listener token registered before bind");
        let manager = manager.clone();
        let runtime = runtime.clone();
        let traffic = traffic.clone();
        tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    _ = stop.notified() => break,
                    accepted = tcp.accept() => accepted,
                };
                let Ok((stream, _)) = accepted else {
                    break;
                };
                let Some(active) = manager.active_snapshot() else {
                    continue;
                };
                if matches!(
                    manager.state().0,
                    xp_proxy_core::control::GenerationState::Draining
                        | xp_proxy_core::control::GenerationState::Stopped
                ) {
                    continue;
                }
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
            // Publish release only after the socket is closed, not just after
            // the accept loop exits. Activation may immediately rebind it.
            drop(tcp);
            let _ = runtime.stop_token(bind);
        });
    }
    Ok(())
}

async fn activate_prepared(
    manager: &GenerationManager,
    runtime: &Arc<ListenerRuntime>,
    traffic: &Arc<TrafficCounter>,
) -> Result<u64> {
    let snapshot = manager
        .prepared_snapshot()
        .ok_or_else(|| anyhow::anyhow!("no prepared generation"))?;
    let result =
        replace_listeners(snapshot, manager, runtime, traffic, || manager.activate()).await;
    if result.is_err() {
        manager.discard_failed_prepare();
    }
    result
}

async fn replace_listeners(
    snapshot: ProxySnapshot,
    manager: &GenerationManager,
    runtime: &Arc<ListenerRuntime>,
    traffic: &Arc<TrafficCounter>,
    commit: impl FnOnce() -> Result<u64, String>,
) -> Result<u64> {
    let previous = manager.active_snapshot();
    if !runtime.stop_all().await {
        manager.stop();
        anyhow::bail!("active listeners did not release before activation");
    }
    let result = async {
        start_snapshot_listeners(snapshot, manager.clone(), runtime.clone(), traffic.clone())
            .await?;
        commit().map_err(anyhow::Error::msg)
    }
    .await;
    if result.is_err() {
        if !runtime.stop_all().await {
            manager.stop();
            anyhow::bail!("failed candidate listeners did not release");
        }
        if let Some(previous) = previous
            && let Err(error) = start_snapshot_listeners(
                previous,
                manager.clone(),
                runtime.clone(),
                traffic.clone(),
            )
            .await
        {
            manager.stop();
            anyhow::bail!(
                "candidate activation failed; previous listener recovery failed: {error}"
            );
        }
    }
    result
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
        let transition = listener_runtime.transition.lock().await;
        let (_, active) = manager.state();
        let is_status = matches!(&request.operation, ControlOperation::Status);
        let result = match request.operation {
            ControlOperation::Prepare { snapshot } => {
                let generation = snapshot.generation;
                manager
                    .prepare(snapshot)
                    .map(|_| generation)
                    .map_err(anyhow::Error::msg)
            }
            ControlOperation::Activate => {
                activate_prepared(&manager, &listener_runtime, &traffic).await
            }
            ControlOperation::Drain => Ok(manager.drain().or(active).unwrap_or(request.generation)),
            ControlOperation::Stop => {
                if !listener_runtime.stop_all().await {
                    anyhow::bail!("active listeners did not release before stop");
                }
                Ok(manager.stop().or(active).unwrap_or(request.generation))
            }
            ControlOperation::Rollback { generation } => {
                if active != Some(generation) {
                    Err(anyhow::anyhow!("rollback generation mismatch"))
                } else if let Some(snapshot) = manager.previous_snapshot() {
                    replace_listeners(snapshot, &manager, &listener_runtime, &traffic, || {
                        manager.rollback(generation)
                    })
                    .await
                } else {
                    Err(anyhow::anyhow!("no previous successful generation"))
                }
            }
            ControlOperation::Status => Ok(active.unwrap_or(request.generation)),
        };
        let error = result.as_ref().err().map(ToString::to_string);
        let (state, generation) = manager.state();
        let response = ControlResponse {
            protocol_version: request.protocol_version,
            request_id: request.request_id,
            generation: generation.or(result.ok()).unwrap_or(request.generation),
            state,
            error,
            metrics: Some(traffic.snapshot()),
            reality_tls_fallback_v1: is_status,
            managed_tls_fallback_ready: if is_status {
                manager.active_managed_tls_fallback_ready()
            } else {
                None
            },
        };
        // The response snapshot is serialized with the operation, but a slow
        // reader must not hold the shared state-transition lock during I/O.
        drop(transition);
        stream.write_all(&encode_frame(&response)?).await?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use x25519_dalek::{PublicKey, StaticSecret};
    use xp_proxy_core::reality::{RealityClientHelloConfig, build_client_hello};

    #[tokio::test]
    async fn outbound_returns_receive_window_for_repeated_and_large_responses() {
        use base64::Engine;
        use xp_proxy_core::control::{
            ListenerSnapshot, RealitySnapshot, UserSnapshot, XmuxSnapshot,
        };
        timeout(Duration::from_secs(5), async {
            let echo = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin = echo.local_addr().unwrap();
            let echo_task = tokio::spawn(async move {
                let (mut tcp, _) = echo.accept().await.unwrap();
                tcp.set_nodelay(true).unwrap();
                for _ in 0..80 {
                    let mut data = [0; 1024];
                    tcp.read_exact(&mut data).await.unwrap();
                    tcp.write_all(&data).await.unwrap();
                }
                tcp.write_all(&vec![77; 128 * 1024]).await.unwrap();
            });
            let remote = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = remote.local_addr().unwrap();
            let private = [9; 32];
            let public = PublicKey::from(&StaticSecret::from(private));
            let encode =
                |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
            let user = uuid::Uuid::new_v4();
            let snapshot = ListenerSnapshot {
                bind: address.to_string(),
                path: "/xp-xhttp".into(),
                xmux: XmuxSnapshot::default(),
                fallbacks: vec![],
                reality_tls_target: None,
                reality: RealitySnapshot {
                    dest: "127.0.0.1:1".into(),
                    server_names: vec!["example.test".into()],
                    short_ids: vec!["aaaaaaaaaaaaaaaa".into()],
                    private_key: encode(&private),
                },
            };
            let server = tokio::spawn(async move {
                let (tcp, _) = remote.accept().await.unwrap();
                handle_connection(
                    tcp,
                    vec![UserSnapshot {
                        uuid: user.to_string(),
                    }],
                    snapshot,
                    Arc::new(TrafficCounter::default()),
                )
                .await
                .unwrap();
            });
            let local = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let mut client = TcpStream::connect(local.local_addr().unwrap())
                .await
                .unwrap();
            client.set_nodelay(true).unwrap();
            let (tcp, _) = local.accept().await.unwrap();
            let config = OutboundConfig {
                listen: local.local_addr().unwrap(),
                remote: address,
                server_name: "example.test".into(),
                server_public_key: encode(public.as_bytes()),
                short_id: "aaaaaaaaaaaaaaaa".into(),
                private_key: encode(&[7; 32]),
                path: "/xp-xhttp".into(),
                uuid: user,
                destination_host: origin.ip().to_string(),
                destination_port: origin.port(),
                xmux: xp_proxy_core::XmuxSettings::default(),
            };
            let pool = Arc::new(outbound::Pool::new(config).unwrap());
            let relay = tokio::spawn(async move { pool.relay(tcp).await });
            for i in 0..80 {
                let data = [i as u8; 1024];
                client.write_all(&data).await.unwrap();
                let mut received = [0; 1024];
                client.read_exact(&mut received).await.unwrap();
                assert_eq!(received, data);
            }
            let mut bulk = vec![0; 128 * 1024];
            client.read_exact(&mut bulk).await.unwrap();
            assert_eq!(bulk, vec![77; 128 * 1024]);
            client.shutdown().await.unwrap();
            relay.await.unwrap().unwrap();
            echo_task.await.unwrap();
            server.abort();
        })
        .await
        .expect("outbound must release receive window beyond 65535 bytes");
    }

    #[test]
    fn split_client_hello_preserves_buffered_tail() {
        let server_private = [9u8; 32];
        let config = RealityClientHelloConfig {
            server_name: "origin.example".into(),
            server_public_key: PublicKey::from(&StaticSecret::from(server_private)).to_bytes(),
            short_id: vec![1, 2, 3, 4, 5, 6, 7, 8],
            unix_seconds: 1_700_000_000,
            version: [1, 8, 0, 0],
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
