use super::*;
use base64::Engine;
use x25519_dalek::{PublicKey, StaticSecret};

async fn session(
    hold_response: bool,
) -> (
    TcpStream,
    tokio::task::JoinHandle<Result<()>>,
    tokio::task::JoinHandle<()>,
) {
    let remote = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = remote.local_addr().unwrap();
    let private = [9; 32];
    let public = PublicKey::from(&StaticSecret::from(private));
    let server = tokio::spawn(async move {
        let (mut tcp, _) = remote.accept().await.unwrap();
        tcp.set_nodelay(true).unwrap();
        let mut header = [0; 5];
        tcp.read_exact(&mut header).await.unwrap();
        let size = u16::from_be_bytes([header[3], header[4]]) as usize;
        let mut hello = header.to_vec();
        hello.resize(5 + size, 0);
        tcp.read_exact(&mut hello[5..]).await.unwrap();
        let parsed = parse_client_hello(&hello).unwrap();
        let authenticated = authenticate_client_hello_with_policy(
            &parsed,
            private,
            &[[0xaa; 8]],
            &["example.test".into()],
            None,
            0,
        )
        .unwrap();
        let tls = reality_tls::accept(hello, Vec::new(), tcp, &authenticated, "example.test")
            .await
            .unwrap();
        let mut connection = h2::server::handshake(tls).await.unwrap();
        let (mut request, mut response) = connection.accept().await.unwrap().unwrap();
        let mut drivers = tokio::task::JoinSet::new();
        drivers.spawn(async move { while connection.accept().await.is_some() {} });
        let data = request.body_mut().data().await.unwrap().unwrap();
        assert!(
            decode_request(&data).is_ok(),
            "actual VLESS request required"
        );
        request
            .body_mut()
            .flow_control()
            .release_capacity(data.len())
            .unwrap();
        let mut send = response
            .send_response(http::Response::new(()), false)
            .unwrap();
        send.send_data(bytes::Bytes::from_static(&[0, 0, 42]), false)
            .unwrap();
        if hold_response {
            // A remote target may remain silent indefinitely after client reset.
            std::future::pending::<()>().await;
        } else {
            while let Some(data) = request.body_mut().data().await {
                let data = data.unwrap();
                request
                    .body_mut()
                    .flow_control()
                    .release_capacity(data.len())
                    .unwrap();
            }
            send.send_data(bytes::Bytes::from_static(&[77]), true)
                .unwrap();
            // Keep the driver alive until the downstream has read its response.
            std::future::pending::<()>().await;
        }
    });
    let local = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = TcpStream::connect(local.local_addr().unwrap())
        .await
        .unwrap();
    let (tcp, _) = local.accept().await.unwrap();
    let encode = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let config = OutboundConfig {
        listen: local.local_addr().unwrap(),
        remote: address,
        server_name: "example.test".into(),
        server_public_key: encode(public.as_bytes()),
        short_id: "aaaaaaaaaaaaaaaa".into(),
        private_key: encode(&[7; 32]),
        path: "/xp-xhttp".into(),
        uuid: uuid::Uuid::new_v4(),
        destination_host: "127.0.0.1".into(),
        destination_port: 80,
        xmux: Default::default(),
    };
    let pool = Arc::new(outbound::Pool::new(config).unwrap());
    (
        client,
        tokio::spawn(async move { pool.relay(tcp).await }),
        server,
    )
}

#[tokio::test]
async fn reset_cancels_silent_outbound_response() {
    timeout(Duration::from_secs(5), async {
        let (mut client, mut relay, server) = session(true).await;
        assert_eq!(client.read_u8().await.unwrap(), 42);
        socket2::SockRef::from(&client)
            .set_linger(Some(Duration::ZERO))
            .unwrap();
        drop(client);
        let result = timeout(Duration::from_millis(150), &mut relay).await;
        server.abort();
        relay.abort();
        assert!(
            result.is_ok(),
            "local reset must cancel a silent remote response"
        );
        assert!(result.unwrap().unwrap().is_err());
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn graceful_upload_eof_preserves_downstream_response() {
    timeout(Duration::from_secs(5), async {
        let (mut client, relay, server) = session(false).await;
        assert_eq!(client.read_u8().await.unwrap(), 42);
        client.shutdown().await.unwrap();
        assert_eq!(client.read_u8().await.unwrap(), 77);
        let mut tail = [0; 1];
        assert_eq!(client.read(&mut tail).await.unwrap(), 0);
        relay.await.unwrap().unwrap();
        server.abort();
    })
    .await
    .unwrap();
}
