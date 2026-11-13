use anyhow::Result;
use clap::Parser;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};
use xp_proxy_core::{
    GenerationManager, ProxySnapshot,
    control::{ControlOperation, ControlRequest, ControlResponse, decode_frame, encode_frame},
};

#[derive(Debug, Parser)]
#[command(name = "xp-proxy")]
struct Args {
    #[arg(long)]
    validate_snapshot: Option<std::path::PathBuf>,
    #[arg(long)]
    control_socket: Option<std::path::PathBuf>,
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
            ControlOperation::Drain => Ok(active.unwrap_or(request.generation)),
            ControlOperation::Stop => Ok(active.unwrap_or(request.generation)),
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
