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
