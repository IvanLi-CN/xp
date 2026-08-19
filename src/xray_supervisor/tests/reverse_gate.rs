use super::*;

async fn start_stats_server(
    listener: tokio::net::TcpListener,
) -> (
    oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
) {
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = tonic::transport::Server::builder()
        .add_service(StatsServiceServer::new(TestStats))
        .serve_with_incoming_shutdown(incoming, async {
            let _ = shutdown_rx.await;
        });
    (shutdown_tx, tokio::spawn(server))
}

#[tokio::test]
async fn transient_xray_loss_requires_reconcile_before_reverse_reopens() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (first_shutdown, first_server) = start_stats_server(listener).await;
    let (tx, mut rx) = mpsc::unbounded_channel::<ReconcileRequest>();
    let reconcile = ReconcileHandle::from_sender(tx);
    let opts = XraySupervisorOptions {
        interval: Duration::from_millis(20),
        fails_before_down: 3,
        connect_timeout: Duration::from_millis(20),
        request_timeout: Duration::from_millis(20),
        down_log_throttle: Duration::from_secs(3600),
        restart_cooldown: Duration::from_secs(3600),
        restart_max_cooldown: Duration::from_secs(3600),
    };
    let (health, task) = spawn_xray_supervisor_with_options(addr, opts, reconcile.clone());

    tokio::time::timeout(Duration::from_secs(2), async {
        while health.snapshot().await.status != XrayStatus::Up {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    reconcile.set_reverse_runtime_ready(true);
    assert!(reconcile.reverse_gate().load(Ordering::Acquire));

    let _ = first_shutdown.send(());
    let _ = first_server.await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while reconcile.reverse_gate().load(Ordering::Acquire) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let (second_shutdown, second_server) = start_stats_server(listener).await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while rx.recv().await != Some(ReconcileRequest::Full) {}
    })
    .await
    .unwrap();
    assert!(!reconcile.reverse_gate().load(Ordering::Acquire));

    let _ = second_shutdown.send(());
    let _ = second_server.await;
    task.abort();
}
