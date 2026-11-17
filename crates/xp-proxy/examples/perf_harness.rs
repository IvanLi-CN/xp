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
    /// Fixed transaction count for functional interop; zero means sustained load.
    #[arg(long, default_value_t = 0)]
    requests_per_client: u64,
    #[arg(long, default_value_t = 64 * 1024)]
    bulk_frame_bytes: usize,
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
        if header[0] == 3 {
            return Ok(());
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
    if args.mode == "tls-origin" {
        use http_body_util::Full;
        use hyper::{Response, service::service_fn};
        use hyper_util::rt::{TokioExecutor, TokioIo};
        use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
        let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519)?;
        let cert = rcgen::CertificateParams::new(vec!["test.local".into()])?.self_signed(&key)?;
        let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
        )?;
        config.alpn_protocols = vec![b"h2".to_vec()];
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind(&args.addr).await?;
        println!("{}", json!({"ready": listener.local_addr()?.to_string()}));
        loop {
            let (tcp, _) = listener.accept().await?;
            tcp.set_nodelay(true)?;
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                if let Ok(tls) = acceptor.accept(tcp).await {
                    let service = service_fn(|_| async {
                        Ok::<_, std::convert::Infallible>(Response::new(Full::new(
                            bytes::Bytes::from_static(b"camouflage fixture"),
                        )))
                    });
                    let _ = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
                        .serve_connection(TokioIo::new(tls), service)
                        .await;
                }
            });
        }
    }
    if args.mode == "crypto" {
        use xp_proxy_core::tls13::{Tls13CipherSuite, Tls13TrafficKey};
        let key = Tls13TrafficKey::from_secret(Tls13CipherSuite::Aes128GcmSha256, &[7u8; 32]);
        let payload = [42u8; 16 * 1024];
        let started = Instant::now();
        let mut records = 0u64;
        while started.elapsed() < Duration::from_millis(args.duration_ms) {
            let wire = key.seal(23, &payload, records)?;
            let (_, decoded) = key.open(&wire, records)?;
            assert_eq!(decoded, payload);
            records += 1;
        }
        println!(
            "{}",
            json!({
                "records": records,
                "seconds": started.elapsed().as_secs_f64(),
                "roundtrip_mbps": records as f64 * 16384.0 * 8.0
                    / started.elapsed().as_secs_f64() / 1e6,
            })
        );
        return Ok(());
    }
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
    let request_limit = args.requests_per_client;
    let operation = match args.workload.as_str() {
        "latency" => 0,
        "upload" => 1,
        "download" => 2,
        _ => bail!("invalid workload"),
    };
    if !(1..=1024 * 1024).contains(&args.bulk_frame_bytes) {
        bail!("bulk frame size must be between 1 and 1048576 bytes");
    }
    let len = if operation == 0 {
        1024
    } else {
        args.bulk_frame_bytes
    };
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
                while tokio::time::Instant::now() < stop
                    && (request_limit == 0 || seq < request_limit)
                {
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
                // End the application session explicitly, independently of each
                // proxy implementation's TCP half-close policy.
                let mut close = [0u8; 17];
                close[0] = 3;
                tcp.write_all(&close).await?;
                let mut tail = [0u8; 1];
                if tcp.read(&mut tail).await? != 0 {
                    bail!("unexpected data after completed session");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn verified_upload_and_explicit_close_release_origin_session() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (server, _) = listener.accept().await.unwrap();
        let task = tokio::spawn(origin(server));
        let mut header = [0u8; 17];
        header[0] = 1;
        header[9..].copy_from_slice(&1024u64.to_be_bytes());
        client.write_all(&header).await.unwrap();
        client.write_all(&[42u8; 1024]).await.unwrap();
        let mut ack = [0u8; 40];
        client.read_exact(&mut ack).await.unwrap();
        assert_eq!(&ack[8..], Sha256::digest([42u8; 1024]).as_slice());
        header[0] = 3;
        client.write_all(&header).await.unwrap();
        assert_eq!(client.read(&mut [0u8; 1]).await.unwrap(), 0);
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn corrupted_upload_never_receives_success_ack() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (server, _) = listener.accept().await.unwrap();
        let task = tokio::spawn(origin(server));
        let mut header = [0u8; 17];
        header[0] = 1;
        header[9..].copy_from_slice(&1u64.to_be_bytes());
        client.write_all(&header).await.unwrap();
        client.write_all(&[43]).await.unwrap();
        assert_eq!(client.read(&mut [0u8; 40]).await.unwrap(), 0);
        assert!(task.await.unwrap().is_err());
    }
}
