use anyhow::Result;
use clap::Parser;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UnixListener, UnixStream},
};
use xp_proxy_core::{
    GenerationManager, ProxySnapshot,
    control::{ControlOperation, ControlRequest, ControlResponse, decode_frame, encode_frame},
    vless::{decode_request, is_authorized},
};

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
    if let Some(path) = args.snapshot {
        let snapshot: ProxySnapshot = serde_json::from_slice(&std::fs::read(path)?)?;
        manager.prepare(snapshot).map_err(anyhow::Error::msg)?;
        manager.activate().map_err(anyhow::Error::msg)?;
        let active = manager.active_snapshot().expect("activated snapshot");
        let users = active.users.clone();
        for listener in active.listeners {
            let users = users.clone();
            let bind = listener.bind.parse::<std::net::SocketAddr>()?;
            let tcp = TcpListener::bind(bind).await?;
            tokio::spawn(async move {
                loop {
                    let Ok((stream, _)) = tcp.accept().await else {
                        break;
                    };
                    let users = users.clone();
                    tokio::spawn(async move {
                        let _ = handle_vless(stream, users).await;
                    });
                }
            });
        }
    }
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let manager = manager.clone();
                tokio::spawn(async move { let _ = handle_control(stream, manager).await; });
            }
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    let _ = std::fs::remove_file(socket);
    Ok(())
}

async fn handle_vless(
    mut inbound: TcpStream,
    users: Vec<xp_proxy_core::control::UserSnapshot>,
) -> Result<()> {
    let mut header = vec![0u8; 8192];
    let used = inbound.read(&mut header).await?;
    if used == 0 {
        anyhow::bail!("empty VLESS request");
    }
    let (request, consumed) =
        decode_request(&header[..used]).map_err(|error| anyhow::anyhow!(error))?;
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
    if consumed < used {
        outbound.write_all(&header[consumed..used]).await?;
    }
    let (mut ri, mut wi) = inbound.into_split();
    let (mut ro, mut wo) = outbound.into_split();
    let uplink = tokio::io::copy(&mut ri, &mut wo);
    let downlink = tokio::io::copy(&mut ro, &mut wi);
    let _ = tokio::try_join!(uplink, downlink)?;
    Ok(())
}

async fn handle_control(mut stream: UnixStream, manager: GenerationManager) -> Result<()> {
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
        let (_, active) = manager.state();
        let result = match request.operation {
            ControlOperation::Prepare { snapshot } => manager
                .prepare(snapshot)
                .map(|_| manager.state().1.unwrap_or(request.generation)),
            ControlOperation::Activate => manager.activate(),
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
        };
        stream.write_all(&encode_frame(&response)?).await?;
    }
}
