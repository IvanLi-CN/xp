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
    #[cfg(test)]
    wait_hook: std::sync::Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
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
            #[cfg(test)]
            wait_hook: std::sync::Mutex::new(None),
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
            tokio::pin!(notified);
            // Register before checking capacity: releases can precede our first poll.
            notified.as_mut().enable();
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
            #[cfg(test)]
            {
                let hook = self.wait_hook.lock().unwrap().clone();
                if let Some(hook) = hook {
                    hook();
                }
            }
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
        let (sender, mut connection) = h2::client::Builder::new()
            .initial_window_size(256 * 1024)
            .initial_connection_window_size(1024 * 1024)
            .handshake(tls)
            .await?;
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn released_slots_wake_all_waiters_before_their_first_notification_poll() {
        let encode = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        let pool = Arc::new(
            Pool::new(OutboundConfig {
                listen: "127.0.0.1:1".parse().unwrap(),
                remote: "127.0.0.1:1".parse().unwrap(),
                server_name: "example.test".into(),
                server_public_key: encode(&[9; 32]),
                private_key: encode(&[7; 32]),
                short_id: "aaaaaaaaaaaaaaaa".into(),
                path: "/xp-xhttp".into(),
                uuid: uuid::Uuid::new_v4(),
                destination_host: "127.0.0.1".into(),
                destination_port: 1,
                xmux: xp_proxy_core::XmuxSettings {
                    max_concurrency: 2,
                    ..Default::default()
                },
            })
            .unwrap(),
        );
        // Admission needs a live sender, but no request or remote protocol traffic.
        let (client, _peer) = tokio::io::duplex(4096);
        let (sender, connection) = h2::client::Builder::new().handshake(client).await.unwrap();
        let driver = tokio::spawn(async move {
            let _ = connection.await;
        });
        let transport = Arc::new(Transport {
            sender,
            active: AtomicUsize::new(2),
            requests: AtomicUsize::new(2),
            created: Instant::now(),
            dead: Arc::new(AtomicBool::new(false)),
            driver: driver.abort_handle(),
        });
        pool.transports.lock().await.push(transport.clone());
        let occupied = [0, 1].map(|_| Lease {
            transport: transport.clone(),
            notify: pool.notify.clone(),
        });
        let checked = Arc::new(std::sync::Barrier::new(3));
        let resume = Arc::new(std::sync::Barrier::new(3));
        *pool.wait_hook.lock().unwrap() = Some(Arc::new({
            let checked = checked.clone();
            let resume = resume.clone();
            let calls = AtomicUsize::new(0);
            move || {
                if calls.fetch_add(1, Ordering::SeqCst) < 2 {
                    checked.wait();
                    resume.wait();
                }
            }
        }));
        let mut waiters = tokio::task::JoinSet::new();
        for _ in 0..2 {
            let pool = pool.clone();
            waiters.spawn(async move { pool.lease().await.unwrap() });
        }
        checked.wait();
        drop(occupied);
        resume.wait();
        let first = timeout(Duration::from_secs(1), waiters.join_next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let second = timeout(Duration::from_secs(1), waiters.join_next())
            .await
            .expect("both free slots must be leased while the first replacement remains active")
            .unwrap()
            .unwrap();
        assert_eq!(transport.active.load(Ordering::SeqCst), 2);
        drop((first, second));
        assert_eq!(transport.active.load(Ordering::SeqCst), 0);
    }

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

    #[tokio::test]
    async fn connection_reuse_one_allows_exactly_one_request_per_transport() {
        pooled_transfer(
            3,
            1024,
            xp_proxy_core::XmuxSettings {
                c_max_reuse_times: 1,
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
