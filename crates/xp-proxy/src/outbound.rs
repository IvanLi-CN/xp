//! One bounded REALITY/H2 transport pool per explicit outbound configuration.
use crate::{OutboundConfig, reality_tls};
use anyhow::{Result, anyhow, bail};
use base64::Engine;
use bytes::Bytes;
use std::{
    future::poll_fn,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::{Mutex, Notify},
};
use xp_proxy_core::{reality::RealityClientHelloConfig, vless::encode_tcp_request};

struct Transport {
    sender: h2::client::SendRequest<Bytes>,
    active: AtomicUsize,
    requests: AtomicUsize,
    created: Instant,
    dead: Arc<AtomicBool>,
    driver: tokio::task::AbortHandle,
}
impl Drop for Transport {
    fn drop(&mut self) {
        self.driver.abort();
    }
}
struct Lease {
    transport: Arc<Transport>,
    notify: Arc<Notify>,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.transport.active.fetch_sub(1, Ordering::SeqCst);
        self.notify.notify_one();
    }
}

pub struct Pool {
    config: OutboundConfig,
    public_key: [u8; 32],
    private_key: [u8; 32],
    short_id: Vec<u8>,
    transports: Mutex<Vec<Arc<Transport>>>,
    notify: Arc<Notify>,
}
impl Pool {
    pub fn new(config: OutboundConfig) -> Result<Self> {
        let decode = |s: &str| -> Result<[u8; 32]> {
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(s)?
                .try_into()
                .map_err(|_| anyhow!("REALITY key must contain exactly 32 bytes"))
        };
        let public_key = decode(&config.server_public_key)?;
        let private_key = decode(&config.private_key)?;
        let short_id = hex::decode(config.short_id.trim_start_matches("0x"))?;
        xp_proxy_core::XhttpConfig {
            path: config.path.clone(),
            mode: "stream-one".into(),
            xmux: config.xmux.clone(),
        }
        .validate()
        .map_err(anyhow::Error::msg)?;
        Ok(Self {
            config,
            public_key,
            private_key,
            short_id,
            transports: Mutex::new(Vec::new()),
            notify: Arc::new(Notify::new()),
        })
    }
    fn reusable(&self, transport: &Transport) -> bool {
        let settings = &self.config.xmux;
        let requests = transport.requests.load(Ordering::SeqCst);
        !transport.dead.load(Ordering::SeqCst)
            && (settings.h_max_request_times == 0
                || requests < settings.h_max_request_times as usize)
            && (settings.c_max_reuse_times == 0 || requests < settings.c_max_reuse_times as usize)
            && (settings.h_max_reusable_secs == 0
                || transport.created.elapsed().as_secs() < settings.h_max_reusable_secs as u64)
    }
    async fn lease(&self) -> Result<Lease> {
        loop {
            let notified = self.notify.notified();
            let mut transports = self.transports.lock().await;
            transports.retain(|t| self.reusable(t) || t.active.load(Ordering::SeqCst) != 0);
            let limit = self.config.xmux.max_concurrency as usize;
            let available = transports.iter().find(|t| {
                self.reusable(t) && (limit == 0 || t.active.load(Ordering::SeqCst) < limit)
            });
            let transport = if let Some(transport) = available {
                Some(transport.clone())
            } else if transports.len() < self.config.xmux.max_connections as usize {
                let transport = Arc::new(self.connect().await?);
                transports.push(transport.clone());
                Some(transport)
            } else {
                None
            };
            if let Some(transport) = transport {
                transport.active.fetch_add(1, Ordering::SeqCst);
                transport.requests.fetch_add(1, Ordering::SeqCst);
                return Ok(Lease {
                    transport,
                    notify: self.notify.clone(),
                });
            }
            drop(transports);
            notified.await;
        }
    }
    async fn connect(&self) -> Result<Transport> {
        let tcp = TcpStream::connect(self.config.remote).await?;
        tcp.set_nodelay(true)?;
        let tls = reality_tls::connect(
            tcp,
            RealityClientHelloConfig {
                server_name: self.config.server_name.clone(),
                server_public_key: self.public_key,
                short_id: self.short_id.clone(),
                unix_seconds: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as u32,
                version: [1, 8, 0, 0],
                alpn: vec!["h2".into()],
            },
            self.private_key,
        )
        .await?;
        let (sender, mut connection) = h2::client::handshake(tls).await?;
        let dead = Arc::new(AtomicBool::new(false));
        let driver_dead = dead.clone();
        let notify = self.notify.clone();
        let period = self.config.xmux.h_keep_alive_period;
        let mut ping = if period > 0 {
            connection.ping_pong()
        } else {
            None
        };
        let driver = tokio::spawn(async move {
            let heartbeat = async move {
                if let Some(ref mut ping) = ping {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(period as u64)).await;
                        tokio::time::timeout(
                            std::time::Duration::from_secs(10),
                            ping.ping(h2::Ping::opaque()),
                        )
                        .await??;
                    }
                } else {
                    std::future::pending::<()>().await;
                }
                #[allow(unreachable_code)]
                Ok::<(), anyhow::Error>(())
            };
            tokio::select! { _ = &mut connection => {}, _ = heartbeat => {} }
            driver_dead.store(true, Ordering::SeqCst);
            notify.notify_one();
        })
        .abort_handle();
        Ok(Transport {
            sender,
            active: AtomicUsize::new(0),
            requests: AtomicUsize::new(0),
            created: Instant::now(),
            dead,
            driver,
        })
    }
    pub async fn relay(&self, local: TcpStream) -> Result<()> {
        let lease = self.lease().await?;
        let mut sender = lease.transport.sender.clone().ready().await?;
        let uri = format!(
            "https://{}{}/?x_padding={}",
            self.config.server_name,
            self.config.path.trim_end_matches('/'),
            "x".repeat(100)
        );
        let request = http::Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/octet-stream")
            .body(())?;
        let (response, mut send) = sender.send_request(request, false)?;
        let header = encode_tcp_request(
            self.config.uuid,
            (&self.config.destination_host, self.config.destination_port),
            &[],
        )?;
        send_bytes(&mut send, Bytes::from(header)).await?;
        let (mut read, mut write) = local.into_split();
        let upload = async move {
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let n = read.read(&mut buffer).await?;
                if n == 0 {
                    break;
                }
                send_bytes(&mut send, Bytes::copy_from_slice(&buffer[..n])).await?;
            }
            send.send_data(Bytes::new(), true)?;
            Ok::<(), anyhow::Error>(())
        };
        let download = async move {
            let response = response.await?;
            if !response.status().is_success() {
                bail!("remote XHTTP request returned {}", response.status());
            }
            let mut body = response.into_body();
            let mut header = Vec::with_capacity(2);
            while let Some(chunk) = body.data().await {
                let chunk = chunk?;
                let mut start = 0;
                while header.len() < 2 && start < chunk.len() {
                    header.push(chunk[start]);
                    start += 1;
                }
                if header.len() == 2 && header != [0, 0] {
                    bail!("invalid VLESS response header");
                }
                write.write_all(&chunk[start..]).await?;
                body.flow_control().release_capacity(chunk.len())?;
            }
            if header.len() != 2 {
                bail!("truncated VLESS response header");
            }
            write.shutdown().await?;
            Ok::<(), anyhow::Error>(())
        };
        // Keep the upload alive across a clean half-close; errors cancel both halves.
        tokio::try_join!(upload, download)?;
        drop(lease);
        Ok(())
    }
}

async fn send_bytes(send: &mut h2::SendStream<Bytes>, mut data: Bytes) -> Result<()> {
    while !data.is_empty() {
        send.reserve_capacity(data.len());
        let capacity = poll_fn(|cx| send.poll_capacity(cx))
            .await
            .ok_or_else(|| anyhow!("H2 stream closed"))??;
        if capacity == 0 {
            continue;
        }
        let chunk = data.split_to(capacity.min(data.len()));
        send.send_data(chunk, false)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        net::TcpListener,
        time::{Duration, timeout},
    };
    use xp_proxy_core::{
        TrafficCounter,
        control::{ListenerSnapshot, RealitySnapshot, UserSnapshot, XmuxSnapshot},
    };

    #[tokio::test]
    async fn pooled_reality_h2_preserves_large_payloads_and_releases_streams() {
        pooled_transfer(8, 1024 * 1024, Default::default(), 1).await;
    }

    #[tokio::test]
    async fn default_pool_accepts_64_concurrent_clients() {
        pooled_transfer(64, 1024, Default::default(), 1).await;
    }

    #[tokio::test]
    async fn pool_retires_exhausted_transports_without_rejecting_clients() {
        pooled_transfer(
            3,
            1024,
            xp_proxy_core::XmuxSettings {
                h_max_request_times: 1,
                ..Default::default()
            },
            3,
        )
        .await;
    }

    async fn pooled_transfer(
        concurrency: usize,
        payload_size: usize,
        xmux: xp_proxy_core::XmuxSettings,
        expected_transports: usize,
    ) {
        timeout(Duration::from_secs(15), async {
            let echo = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin = echo.local_addr().unwrap();
            let echo_task = tokio::spawn(async move {
                loop {
                    let (tcp, _) = echo.accept().await.unwrap();
                    tokio::spawn(async move {
                        let (mut read, mut write) = tcp.into_split();
                        tokio::io::copy(&mut read, &mut write).await.unwrap();
                    });
                }
            });
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let remote = listener.local_addr().unwrap();
            let private = [9u8; 32];
            let public = x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(private));
            let encode =
                |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
            let user = uuid::Uuid::new_v4();
            let snapshot = ListenerSnapshot {
                bind: remote.to_string(),
                path: "/xp-xhttp".into(),
                xmux: XmuxSnapshot::default(),
                fallbacks: vec![],
                reality: RealitySnapshot {
                    dest: "127.0.0.1:1".into(),
                    server_names: vec!["example.test".into()],
                    short_ids: vec!["aaaaaaaaaaaaaaaa".into()],
                    private_key: encode(&private),
                },
            };
            let accepted = Arc::new(AtomicUsize::new(0));
            let count = accepted.clone();
            let server = tokio::spawn(async move {
                loop {
                    let (tcp, _) = listener.accept().await.unwrap();
                    count.fetch_add(1, Ordering::SeqCst);
                    let snapshot = snapshot.clone();
                    tokio::spawn(async move {
                        crate::handle_connection(
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
                }
            });
            let local = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let local_addr = local.local_addr().unwrap();
            let pool = Arc::new(
                Pool::new(OutboundConfig {
                    listen: local_addr,
                    remote,
                    server_name: "example.test".into(),
                    server_public_key: encode(public.as_bytes()),
                    short_id: "aaaaaaaaaaaaaaaa".into(),
                    private_key: encode(&[7u8; 32]),
                    path: "/xp-xhttp".into(),
                    uuid: user,
                    destination_host: origin.ip().to_string(),
                    destination_port: origin.port(),
                    xmux,
                })
                .unwrap(),
            );
            let shared = pool.clone();
            let relay = tokio::spawn(async move {
                let mut tasks = Vec::new();
                for _ in 0..concurrency {
                    let (tcp, _) = local.accept().await.unwrap();
                    tcp.set_nodelay(true).unwrap();
                    let pool = shared.clone();
                    tasks.push(tokio::spawn(async move {
                        pool.relay(tcp).await.unwrap();
                    }));
                }
                for task in tasks {
                    task.await.unwrap();
                }
            });
            let mut clients = Vec::new();
            for _ in 0..concurrency {
                clients.push(tokio::spawn(async move {
                    let tcp = TcpStream::connect(local_addr).await.unwrap();
                    let (mut read, mut write) = tcp.into_split();
                    let upload = async {
                        write.write_all(&vec![42u8; payload_size]).await.unwrap();
                        write.shutdown().await.unwrap();
                    };
                    let download = async {
                        let mut bytes = Vec::new();
                        read.read_to_end(&mut bytes).await.unwrap();
                        assert_eq!(bytes, vec![42u8; payload_size]);
                    };
                    tokio::join!(upload, download);
                }));
            }
            for client in clients {
                client.await.unwrap();
            }
            relay.await.unwrap();
            assert_eq!(
                accepted.load(Ordering::SeqCst),
                expected_transports,
                "pool must respect transport reuse limits"
            );
            let transports = pool.transports.lock().await;
            assert_eq!(transports.len(), 1);
            assert_eq!(transports[0].active.load(Ordering::SeqCst), 0);
            server.abort();
            echo_task.abort();
        })
        .await
        .expect("large bidirectional H2 traffic must not stall on flow control");
    }
}
