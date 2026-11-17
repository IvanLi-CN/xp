use super::*;
use base64::Engine;
use sha2::{Digest, Sha256};
use std::net::{IpAddr, SocketAddr};
use xp_proxy_core::control::{ListenerSnapshot, RealitySnapshot, UserSnapshot, XmuxSnapshot};

fn listener_snapshot(bind: SocketAddr) -> ListenerSnapshot {
    ListenerSnapshot {
        bind: bind.to_string(),
        path: "/xp-xhttp".into(),
        reality: RealitySnapshot {
            dest: "127.0.0.1:1".into(),
            server_names: vec!["test.local".into()],
            short_ids: vec!["0123456789abcdef".into()],
            private_key: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([9; 32]),
        },
        xmux: XmuxSnapshot::default(),
        fallbacks: vec![],
    }
}

#[tokio::test]
async fn reality_xhttp_and_raw_vless_cover_all_four_address_family_pairs() {
    timeout(Duration::from_secs(15), async {
        for raw in [true, false] {
            for ingress in ["127.0.0.1", "::1"] {
                for destination in ["127.0.0.1", "::1"] {
                    transfer(raw, ingress, destination).await;
                }
            }
        }
    })
    .await
    .expect("IPv4/IPv6 transfers must complete without skipping any family");
}

async fn transfer(raw: bool, ingress: &str, destination: &str) {
    const SIZE: usize = 128 * 1024;
    let origin = TcpListener::bind((destination, 0)).await.unwrap();
    let target = origin.local_addr().unwrap();
    let origin_task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = origin.accept().await.unwrap();
            tokio::spawn(async move {
                let mut data = vec![0; SIZE];
                stream.read_exact(&mut data).await.unwrap();
                stream.write_all(&data).await.unwrap();
            });
        }
    });
    let remote = listen::bind("0.0.0.0:0".parse().unwrap()).unwrap();
    let address = SocketAddr::new(
        ingress.parse::<IpAddr>().unwrap(),
        remote.local_addr().unwrap().port(),
    );
    let user = uuid::Uuid::new_v4();
    let snapshot = listener_snapshot(address);
    let server = tokio::spawn(async move {
        loop {
            let (stream, peer) = remote.accept().await.unwrap();
            assert_eq!(peer.ip().to_canonical().is_ipv4(), address.is_ipv4());
            let snapshot = snapshot.clone();
            tokio::spawn(async move {
                handle_connection(
                    stream,
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
    let encode = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let private = x25519_dalek::StaticSecret::from([9; 32]);
    let public = x25519_dalek::PublicKey::from(&private);
    let pool = Arc::new(
        outbound::Pool::new(OutboundConfig {
            listen: "0.0.0.0:0".parse().unwrap(),
            remote: address,
            server_name: "test.local".into(),
            server_public_key: encode(public.as_bytes()),
            short_id: "0123456789abcdef".into(),
            private_key: encode(&[7; 32]),
            path: "/xp-xhttp".into(),
            uuid: user,
            destination_host: destination.into(),
            destination_port: target.port(),
            xmux: xp_proxy_core::XmuxSettings::default(),
        })
        .unwrap(),
    );
    let local = listen::bind("0.0.0.0:0".parse().unwrap()).unwrap();
    let local_address = SocketAddr::new(address.ip(), local.local_addr().unwrap().port());
    let relay = tokio::spawn(async move {
        loop {
            let (stream, _) = local.accept().await.unwrap();
            let pool = pool.clone();
            tokio::spawn(async move {
                pool.relay(stream).await.unwrap();
            });
        }
    });
    // A second round exercises H2 reuse and fresh local connections.
    for _ in 0..2 {
        let mut clients = Vec::new();
        for index in 0..8 {
            clients.push(tokio::spawn(async move {
                let mut stream = TcpStream::connect(if raw { address } else { local_address })
                    .await
                    .unwrap();
                if raw {
                    let header = xp_proxy_core::vless::encode_tcp_request(
                        user,
                        (&target.ip().to_string(), target.port()),
                        &[],
                    )
                    .unwrap();
                    stream.write_all(&header).await.unwrap();
                    let mut response = [0; 2];
                    stream.read_exact(&mut response).await.unwrap();
                    assert_eq!(response, [0, 0]);
                }
                let data = (0..SIZE)
                    .map(|i| (i.wrapping_mul(31) + index) as u8)
                    .collect::<Vec<_>>();
                stream.write_all(&data).await.unwrap();
                let mut received = vec![0; SIZE];
                stream.read_exact(&mut received).await.unwrap();
                assert_eq!(Sha256::digest(&received), Sha256::digest(&data));
            }));
        }
        for client in clients {
            client.await.unwrap();
        }
    }
    relay.abort();
    server.abort();
    origin_task.abort();
}

#[tokio::test]
async fn failed_dual_stack_generation_preserves_previous_listener_and_port_ownership() {
    timeout(Duration::from_secs(5), async {
        let reservation = listen::bind("0.0.0.0:0".parse().unwrap()).unwrap();
        let port = reservation.local_addr().unwrap().port();
        drop(reservation);
        let manager = GenerationManager::default();
        let runtime = Arc::new(ListenerRuntime::default());
        let traffic = Arc::new(TrafficCounter::default());
        let snapshot = ProxySnapshot {
            protocol_version: CONTROL_PROTOCOL_VERSION,
            generation: 1,
            users: vec![UserSnapshot {
                uuid: uuid::Uuid::new_v4().to_string(),
            }],
            listeners: vec![listener_snapshot(SocketAddr::from(([0, 0, 0, 0], port)))],
        };
        manager.prepare(snapshot.clone()).unwrap();
        activate_prepared(&manager, &runtime, &traffic)
            .await
            .unwrap();
        let occupied = listen::bind("0.0.0.0:0".parse().unwrap()).unwrap();
        let mut candidate = snapshot.clone();
        candidate.generation = 2;
        candidate
            .listeners
            .push(listener_snapshot(SocketAddr::from((
                [0, 0, 0, 0],
                occupied.local_addr().unwrap().port(),
            ))));
        manager.prepare(candidate).unwrap();
        assert!(
            activate_prepared(&manager, &runtime, &traffic)
                .await
                .is_err()
        );
        assert_eq!(manager.active_snapshot().unwrap().generation, 1);
        for host in ["127.0.0.1", "::1"] {
            TcpStream::connect((host, port)).await.unwrap();
        }
        assert!(runtime.stop_all().await);
        listen::bind(SocketAddr::from(([0, 0, 0, 0], port))).unwrap();
    })
    .await
    .expect("failed activation must recover the previous dual-stack listener");
}
