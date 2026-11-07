use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn assert_stalled_download_fails(send_partial_body: bool) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/asset", listener.local_addr().unwrap());
    let (stalled_tx, stalled_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(socket.read_u8().await.unwrap());
        }
        if send_partial_body {
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc")
                .await
                .unwrap();
        }
        stalled_tx.send(()).unwrap();
        std::future::pending::<()>().await;
        drop(socket);
    });
    let temp = tempfile::tempdir().unwrap();
    let dest = temp.path().join("asset");
    fs::write(&dest, b"previous verified asset").unwrap();
    let output = dest.clone();
    let download = tokio::spawn(async move { download_to_path(&url, &output).await });
    stalled_rx.await.unwrap();
    if send_partial_body {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if fs::metadata(tmp_path_next_to(&dest)).is_ok_and(|file| file.len() == 3) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the client must consume the first body chunk before pausing time");
    }
    // Pause only after the real HTTP request reaches the peer, avoiding virtual-time
    // races during loopback connection establishment.
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(31)).await;
    let result = tokio::time::timeout(Duration::from_secs(1), download).await;
    server.abort();
    let error = result
        .expect("a stalled download must return within its idle budget")
        .unwrap()
        .expect_err("incomplete HTTP response must fail");
    assert!(format!("{error:#}").contains("timed out"), "{error:#}");
    assert_eq!(fs::read(&dest).unwrap(), b"previous verified asset");
    assert_eq!(
        fs::read_dir(temp.path()).unwrap().count(),
        1,
        "failed downloads must not leave partial staging files"
    );
}

#[tokio::test]
async fn upgrade_download_bounds_stalled_headers() {
    assert_stalled_download_fails(false).await;
}

#[tokio::test]
async fn upgrade_download_bounds_stalled_body_and_cleans_partial_file() {
    assert_stalled_download_fails(true).await;
}

#[tokio::test]
async fn upgrade_download_total_budget_bounds_a_continuously_trickling_body() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/asset", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(socket.read_u8().await.unwrap());
        }
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\na")
            .await
            .unwrap();
        loop {
            tokio::time::sleep(Duration::from_secs(20)).await;
            if socket.write_all(b"a").await.is_err() {
                break;
            }
        }
    });
    let temp = tempfile::tempdir().unwrap();
    let dest = temp.path().join("asset");
    let output = dest.clone();
    let download = tokio::spawn(async move { download_to_path(&url, &output).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while !fs::metadata(tmp_path_next_to(&dest)).is_ok_and(|file| file.len() == 1) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    tokio::time::pause();
    let started = tokio::time::Instant::now();
    let result = tokio::time::timeout(Duration::from_secs(15 * 60 + 1), download).await;
    server.abort();
    let error = result
        .expect("periodic body traffic must not bypass the total budget")
        .unwrap()
        .expect_err("unfinished asset must fail");
    assert!(format!("{error:#}").contains("timed out"), "{error:#}");
    assert!(
        started.elapsed() >= Duration::from_secs(14 * 60),
        "must exercise the total, not idle, timeout"
    );
    assert!(!dest.exists());
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}
