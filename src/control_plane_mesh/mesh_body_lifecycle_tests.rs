use super::*;
use tokio::sync::oneshot;

#[tokio::test]
async fn zero_length_mesh_response_releases_gate_guard_after_eof() {
    use futures_util::StreamExt;

    let gate_lock = Arc::new(tokio::sync::RwLock::new(()));
    let gate_guard = gate_lock.clone().read_owned().await;
    let response = reqwest::Response::from(
        axum::http::Response::builder()
            .status(reqwest::StatusCode::OK)
            .body(reqwest::Body::from(Vec::<u8>::new()))
            .expect("synthetic response"),
    );
    let response = super::reverse::attach_mesh_gate(
        response,
        gate_guard,
        Instant::now() + Duration::from_secs(1),
    );
    let mut body = response.bytes_stream();
    assert!(body.next().await.is_none());
    tokio::time::timeout(Duration::from_millis(100), gate_lock.write_owned())
        .await
        .expect("zero-length response must release the gate guard");
}

#[tokio::test]
async fn zero_length_mesh_response_waits_for_delayed_eof() {
    use futures_util::StreamExt;

    let gate_lock = Arc::new(tokio::sync::RwLock::new(()));
    let gate_guard = gate_lock.clone().read_owned().await;
    let (release_sender, release_receiver) = oneshot::channel();
    let response = reqwest::Response::from(
        axum::http::Response::builder()
            .header(reqwest::header::CONTENT_LENGTH, "0")
            .body(reqwest::Body::wrap_stream(futures_util::stream::once(
                async move {
                    release_receiver
                        .await
                        .map_err(|_| std::io::Error::other("release dropped"))?;
                    Ok::<_, std::io::Error>(bytes::Bytes::new())
                },
            )))
            .expect("synthetic delayed response"),
    );
    let response = super::reverse::attach_mesh_gate(
        response,
        gate_guard,
        Instant::now() + Duration::from_secs(1),
    );
    let mut body = response.bytes_stream();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), gate_lock.clone().write_owned())
            .await
            .is_err(),
        "content length must not release the gate before EOF"
    );
    release_sender
        .send(())
        .expect("delayed body is still waiting");
    while body.next().await.is_some() {}
    tokio::time::timeout(Duration::from_millis(100), gate_lock.write_owned())
        .await
        .expect("delayed EOF must release the gate guard");
}

#[tokio::test(start_paused = true)]
async fn mesh_response_body_first_byte_deadline_releases_stalled_response() {
    use futures_util::StreamExt;

    let gate_lock = Arc::new(tokio::sync::RwLock::new(()));
    let gate_guard = gate_lock.clone().read_owned().await;
    let response = reqwest::Response::from(
        axum::http::Response::builder()
            .status(reqwest::StatusCode::OK)
            .body(reqwest::Body::wrap_stream(futures_util::stream::pending::<
                Result<bytes::Bytes, std::io::Error>,
            >()))
            .expect("synthetic response"),
    );
    let response = super::reverse::attach_mesh_gate_with_body_lease(
        response,
        gate_guard,
        Instant::now() + Duration::from_secs(3),
        Duration::from_secs(15 * 60),
        None,
    );
    let mut body = response.bytes_stream();
    tokio::task::yield_now().await;

    tokio::time::advance(Duration::from_secs(3)).await;
    tokio::task::yield_now().await;
    assert!(
        body.next()
            .await
            .expect("first-byte deadline should emit an error")
            .is_err(),
        "the first body byte must remain bounded by the request deadline"
    );
    assert!(
        gate_lock.clone().try_write_owned().is_ok(),
        "first-byte deadline must release the gate guard"
    );
}

#[tokio::test(start_paused = true)]
async fn mesh_response_body_stream_lease_outlives_admission_slice() {
    use futures_util::StreamExt;
    use tokio::sync::oneshot;

    let gate_lock = Arc::new(tokio::sync::RwLock::new(()));
    let gate_guard = gate_lock.clone().read_owned().await;
    let body = futures_util::stream::once(async {
        Ok::<_, std::io::Error>(bytes::Bytes::from_static(b"first"))
    })
    .chain(futures_util::stream::pending());
    let response = reqwest::Response::from(
        axum::http::Response::builder()
            .status(reqwest::StatusCode::OK)
            .body(reqwest::Body::wrap_stream(body))
            .expect("synthetic response"),
    );
    let (finish_tx, finish_rx) = oneshot::channel();
    let response = super::reverse::attach_mesh_gate_with_body_lease(
        response,
        gate_guard,
        Instant::now() + Duration::from_secs(3),
        Duration::from_secs(15 * 60),
        Some(Box::new(move |outcome| {
            let _ = finish_tx.send(outcome);
        })),
    );
    let mut body = response.bytes_stream();
    assert!(body.next().await.expect("first body chunk").is_ok());

    tokio::time::advance(Duration::from_secs(15 * 60)).await;
    tokio::task::yield_now().await;
    assert!(
        body.next().await.is_none(),
        "stream lease expiry must close the current SSE body cleanly"
    );
    assert_eq!(
        finish_rx.await.expect("lease completion callback"),
        crate::mesh_gate_body::BodyFinish::LeaseExpired
    );
    assert!(
        gate_lock.clone().try_write_owned().is_ok(),
        "stream lease expiry must release the gate guard"
    );
}
