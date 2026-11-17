//! Test-only framed load generator. Never installed as a production binary.
use anyhow::{Result, bail};
use clap::Parser;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Mutex,
};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    mode: String,
    #[arg(long)]
    addr: String,
    #[arg(long, default_value_t = 1)]
    concurrency: usize,
    #[arg(long, default_value_t = 2000)]
    duration_ms: u64,
    #[arg(long, default_value = "latency")]
    workload: String,
}
// The peer validates all bytes before acknowledging sequence + content digest.
async fn origin(mut tcp: TcpStream) -> Result<()> {
    tcp.set_nodelay(true)?;
    loop {
        let mut header = [0u8; 17];
        match tcp.read_exact(&mut header).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e.into()),
        }
        let len = u64::from_be_bytes(header[9..17].try_into()?) as usize;
        if len > 1024 * 1024 {
            bail!("oversized frame");
        }
        let mut data = vec![42u8; len];
        if header[0] != 2 {
            tcp.read_exact(&mut data).await?;
        }
        if data.iter().any(|&b| b != 42) {
            bail!("payload mismatch");
        }
        if header[0] != 1 {
            tcp.write_all(&data).await?;
        }
        let digest = Sha256::digest(&data);
        tcp.write_all(&header[1..9]).await?;
        tcp.write_all(&digest).await?;
        if header[0] == 2 {
            let mut ack = [0u8; 40];
            tcp.read_exact(&mut ack).await?;
            if ack[..8] != header[1..9] || ack[8..] != digest[..] {
                bail!("download ACK mismatch");
            }
        }
    }
}
#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if args.mode == "origin" {
        let listener = TcpListener::bind(&args.addr).await?;
        println!("{}", json!({"ready":listener.local_addr()?.to_string()}));
        loop {
            let (tcp, _) = listener.accept().await?;
            tokio::spawn(async move {
                if let Err(e) = origin(tcp).await {
                    eprintln!("origin: {e}");
                }
            });
        }
    }
    if args.mode != "load" || args.duration_ms < 300 || args.concurrency == 0 {
        bail!("invalid load configuration");
    }
    let operation = match args.workload.as_str() {
        "latency" => 0,
        "upload" => 1,
        "download" => 2,
        _ => bail!("invalid workload"),
    };
    let len = if operation == 0 { 1024 } else { 1024 * 1024 };
    let data = Arc::new(vec![42u8; len]);
    let digest: [u8; 32] = Sha256::digest(&*data).into();
    let start = Instant::now();
    let deadline = tokio::time::Instant::now() + Duration::from_millis(args.duration_ms);
    let stop = deadline - Duration::from_millis(200);
    let samples = Arc::new(Mutex::new(Vec::<f64>::new()));
    let mut tasks = Vec::new();
    for _ in 0..args.concurrency {
        let addr = args.addr.clone();
        let data = data.clone();
        let samples = samples.clone();
        tasks.push(tokio::spawn(async move {
            let work = async {
                let mut tcp = TcpStream::connect(addr).await?;
                tcp.set_nodelay(true)?;
                let mut seq = 0u64;
                let mut received = vec![0u8; len];
                while tokio::time::Instant::now() < stop {
                    let tick = Instant::now();
                    let mut header = [0u8; 17];
                    header[0] = operation;
                    header[1..9].copy_from_slice(&seq.to_be_bytes());
                    header[9..].copy_from_slice(&(len as u64).to_be_bytes());
                    tcp.write_all(&header).await?;
                    if operation != 2 {
                        for chunk in data.chunks(64 * 1024) {
                            tcp.write_all(chunk).await?;
                        }
                    }
                    if operation != 1 {
                        tcp.read_exact(&mut received).await?;
                        if Sha256::digest(&received)[..] != digest {
                            bail!("received digest mismatch");
                        }
                    }
                    let mut ack = [0u8; 40];
                    tcp.read_exact(&mut ack).await?;
                    if ack[..8] != seq.to_be_bytes() || ack[8..] != digest {
                        bail!("ACK mismatch");
                    }
                    if operation == 2 {
                        tcp.write_all(&ack).await?;
                    }
                    samples
                        .lock()
                        .await
                        .push(tick.elapsed().as_secs_f64() * 1000.0);
                    seq += 1;
                }
                tcp.shutdown().await?;
                Ok::<_, anyhow::Error>(())
            };
            match tokio::time::timeout_at(deadline, work).await {
                Ok(Ok(())) => None,
                Ok(Err(e)) => Some(e.to_string()),
                Err(_) => Some("incomplete_at_slot_deadline".into()),
            }
        }));
    }
    let mut errors = Vec::new();
    for task in tasks {
        if let Some(e) = task.await? {
            errors.push(e);
        }
    }
    let elapsed = start.elapsed().as_secs_f64();
    let mut samples = samples.lock().await;
    samples.sort_by(f64::total_cmp);
    let percentile = |p: f64| {
        samples
            .get(((samples.len().saturating_sub(1)) as f64 * p).ceil() as usize)
            .copied()
    };
    println!(
        "{}",
        json!({
            "samples": samples.len(),
            "confirmed_bytes": samples.len()*len,
            "seconds": elapsed,
            "mbps": samples.len() as f64*len as f64*8.0/elapsed/1e6,
            "p50_ms": percentile(0.5),
            "p95_ms": percentile(0.95),
            "errors": errors,
            "incomplete": errors.iter()
                .filter(|s| s.as_str()=="incomplete_at_slot_deadline").count()
        })
    );
    Ok(())
}
