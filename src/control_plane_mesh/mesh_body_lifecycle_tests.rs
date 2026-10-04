use super::*;
use crate::reconcile::ReconcileHandle;
use tokio::sync::oneshot;

struct DropNotifies(Option<oneshot::Sender<()>>);

impl futures_util::Stream for DropNotifies {
    type Item = Result<bytes::Bytes, std::io::Error>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::task::Poll::Pending
    }
}

impl Drop for DropNotifies {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

#[tokio::test]
async fn completed_empty_mesh_body_releases_guard_without_another_poll() {
    let gate_lock = Arc::new(tokio::sync::RwLock::new(()));
    let gate_guard = gate_lock.clone().read_owned().await;
    let response = reqwest::Response::from(
        axum::http::Response::builder()
            .status(reqwest::StatusCode::OK)
            .body(reqwest::Body::from(Vec::<u8>::new()))
            .expect("completed empty response"),
    );
    let (finished_tx, mut finished_rx) = oneshot::channel();
    let response = super::reverse::attach_mesh_gate_with_finish(
        response,
        gate_guard,
        Instant::now() + Duration::from_secs(1),
        Some(Box::new(move |finish| {
            let _ = finished_tx.send(finish);
        })),
    );

    assert!(
        gate_lock.clone().try_write_owned().is_ok(),
        "a body already at EOF must not retain a read guard until the caller polls it"
    );
    assert_eq!(
        finished_rx.try_recv().expect("EOF completion"),
        crate::mesh_gate_body::BodyFinish::Complete
    );
    assert!(response.bytes().await.expect("empty body").is_empty());
}

#[tokio::test]
async fn completed_empty_leased_mesh_body_finishes_without_caller_polling() {
    let gate_lock = Arc::new(tokio::sync::RwLock::new(()));
    let gate_guard = gate_lock.clone().read_owned().await;
    let response = reqwest::Response::from(
        axum::http::Response::builder()
            .status(reqwest::StatusCode::OK)
            .body(reqwest::Body::from(Vec::<u8>::new()))
            .expect("completed empty response"),
    );
    let (finished_tx, mut finished_rx) = oneshot::channel();
    let response = super::reverse::attach_mesh_gate_with_body_lease(
        response,
        gate_guard,
        Instant::now() + Duration::from_secs(3),
        Duration::from_secs(15 * 60),
        Some(Box::new(move |finish| {
            let _ = finished_tx.send(finish);
        })),
    );

    assert!(
        gate_lock.clone().try_write_owned().is_ok(),
        "confirmed EOF must release a leased guard without caller polling"
    );
    assert_eq!(
        finished_rx
            .try_recv()
            .expect("immediate leased EOF completion"),
        crate::mesh_gate_body::BodyFinish::Complete
    );
    assert!(
        response
            .bytes()
            .await
            .expect("empty leased body")
            .is_empty()
    );
}

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

    tokio::task::yield_now().await;
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

#[tokio::test(start_paused = true)]
async fn mesh_response_body_lease_drops_unpolled_upstream_on_expiry() {
    use futures_util::StreamExt;

    let gate_lock = Arc::new(tokio::sync::RwLock::new(()));
    let gate_guard = gate_lock.clone().read_owned().await;
    let (dropped_tx, mut dropped_rx) = oneshot::channel();
    let body = futures_util::stream::once(async {
        Ok::<_, std::io::Error>(bytes::Bytes::from_static(b"first"))
    })
    .chain(DropNotifies(Some(dropped_tx)));
    let response = reqwest::Response::from(
        axum::http::Response::builder()
            .status(reqwest::StatusCode::OK)
            .body(reqwest::Body::wrap_stream(body))
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
    assert!(body.next().await.expect("first body chunk").is_ok());

    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(15 * 60)).await;
    for _ in 0..8 {
        tokio::task::yield_now().await;
        if dropped_rx.try_recv().is_ok() {
            assert!(
                gate_lock.clone().try_write_owned().is_ok(),
                "lease expiry must release the gate without another body poll"
            );
            return;
        }
    }
    panic!("lease expiry must cancel and drop an upstream body that is no longer polled");
}

#[tokio::test]
async fn reenable_preflight_waits_for_snapshot_reservation_before_dispatch() {
    let reconcile = ReconcileHandle::noop();
    assert!(
        reconcile
            .initialize_mesh_gate_until(false, Instant::now() + Duration::from_secs(1))
            .await
    );
    let (mesh_base_url, mesh_requests, mesh_task) =
        super::peer_target_tests::spawn_stalling_mesh().await;
    let peer = super::peer_target_tests::primary_reverse_target(
        Some(mesh_base_url),
        xp_test_fixtures::primary_api_url().to_owned(),
    );
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let client = MeshAwareHttpClient::new(reqwest::Client::new())
        .with_mesh_gate_epoch(reconcile.mesh_gate(), reconcile.mesh_gate_epoch())
        .with_mesh_gate_lock(reconcile.mesh_gate_lock())
        .with_mesh_epoch_barrier(reconcile.mesh_epoch_barrier())
        .with_snapshot_install_reservation(reconcile.snapshot_installing());
    let reservation = reconcile
        .begin_snapshot_install_until(Instant::now() + Duration::from_secs(1))
        .await
        .expect("snapshot should reserve Mesh admission");
    let request = MeshRequest {
        method: reqwest::Method::GET,
        path_and_query: "/api/admin/_internal/mesh/health".to_owned(),
        content_type: None,
        body: Vec::new(),
        total_budget: Duration::from_millis(200),
        allow_ambiguous_fallback: false,
        request_id: "reenable-preflight-snapshot-reservation".to_owned(),
        route: InternalRoute::HealthV2,
        cluster_id: xp_test_fixtures::cluster_fixture53().to_owned(),
        sender_id: xp_test_fixtures::tertiary_node_id().to_owned(),
        updates_active_path: false,
    };

    let error = client
        .send_peer_direct_preflight_for_reenable(&peer, request.clone(), &ca.key_pem, &ca.cert_pem)
        .await
        .expect_err("snapshot reservation must reject re-enable preflight before dispatch");
    assert!(
        matches!(error, MeshRequestError::PreDispatchTimeout),
        "reservation rejection should be known-not-dispatched, got {error:?}"
    );
    assert_eq!(
        mesh_requests.load(Ordering::SeqCst),
        0,
        "reservation-time preflight must be known not dispatched"
    );

    drop(reservation);
    let _ = client
        .send_peer_direct_preflight_for_reenable(&peer, request, &ca.key_pem, &ca.cert_pem)
        .await;
    tokio::time::timeout(Duration::from_secs(1), async {
        while mesh_requests.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("re-enable preflight should resume after reservation release");
    mesh_task.abort();
}
