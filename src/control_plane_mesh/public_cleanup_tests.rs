use super::*;

#[tokio::test]
async fn public_failure_cleanup_converges_after_request_deadline() {
    let client = MeshAwareHttpClient::new(reqwest::Client::new());
    let circuits = client.circuits.clone();
    let public_peers = circuits.hold_public_peers_for_test().await;
    client.defer_public_failure("peer", circuits.next_operation(), None);
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(25), circuits.public_state("peer"))
            .await
            .is_err(),
        "public cleanup should remain pending while its state lock is held"
    );
    drop(public_peers);
    tokio::time::timeout(Duration::from_secs(1), async {
        while circuits.public_state("peer").await != BreakerState::Open {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("bounded public failure cleanup should record the breaker state");
}

#[tokio::test]
async fn delayed_public_cleanup_cannot_clear_a_new_probe_token() {
    let client = MeshAwareHttpClient::new(reqwest::Client::new());
    let peer_id = xp_test_fixtures::primary_node_id();
    client.circuits.record_public_failure(peer_id).await;
    client
        .circuits
        .set_public_probe_ready_for_test(peer_id)
        .await;
    let release = occupy_completion_window(&client).await;
    client.defer_public_failure(peer_id, client.circuits.next_operation(), None);
    let (decision, token) = client
        .circuits
        .before_public_attempt_with_probe_with_token(peer_id, true)
        .await;
    assert_eq!(decision, circuit::MeshAttemptDecision::Probe);
    let token = token.expect("new probe token");
    let mut guard = client
        .public_probe_guard(peer_id, decision, Some(token))
        .expect("probe guard");
    let (drained_tx, drained_rx) = tokio::sync::oneshot::channel();
    client.dispatch_critical_completion("probe-fence", async move {
        tokio::task::yield_now().await;
        let _ = drained_tx.send(());
    });
    release.add_permits(32);
    tokio::time::timeout(Duration::from_secs(1), drained_rx)
        .await
        .expect("cleanup window recovered")
        .expect("cleanup fence");
    assert!(
        client
            .circuits
            .release_public_half_open_probe(peer_id, token)
            .await,
        "a deferred older outcome must not clear the newer probe token"
    );
    guard.disarm();
}

#[tokio::test]
async fn failed_public_probe_is_released_when_its_cleanup_expires() {
    let client = MeshAwareHttpClient::new(reqwest::Client::new())
        .with_mesh_gate(Arc::new(AtomicBool::new(false)));
    let peer_id = xp_test_fixtures::primary_node_id();
    client.circuits.record_public_failure(peer_id).await;
    client
        .circuits
        .set_public_probe_ready_for_test(peer_id)
        .await;
    let (held_tx, mut held_rx) = tokio::sync::mpsc::unbounded_channel();
    let circuits = client.circuits.clone();
    let router = axum::Router::new().fallback(axum::routing::any(move || {
        let circuits = circuits.clone();
        let held_tx = held_tx.clone();
        async move {
            held_tx
                .send(circuits.hold_public_peers_for_test().await)
                .expect("contended Public state receiver");
            axum::http::StatusCode::OK
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("listener address");
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    let peer = super::peer_target_tests::primary_reverse_target(None, format!("http://{address}"));
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let result = client
        .send_peer_request(
            &peer,
            MeshRequest {
                method: reqwest::Method::GET,
                path_and_query: "/api/admin/_internal/mesh/health".to_owned(),
                content_type: None,
                body: Vec::new(),
                total_budget: Duration::from_millis(200),
                allow_ambiguous_fallback: true,
                request_id: "public-cleanup-probe-expiry".to_owned(),
                route: InternalRoute::HealthV2,
                cluster_id: xp_test_fixtures::cluster_fixture53().to_owned(),
                sender_id: xp_test_fixtures::tertiary_node_id().to_owned(),
                updates_active_path: false,
            },
            &ca.key_pem,
            &ca.cert_pem,
        )
        .await
        .expect_err("unsigned Public response must fail closed");
    assert!(matches!(result, MeshRequestError::Protocol(_)));
    let held = held_rx
        .recv()
        .await
        .expect("Public state held after admission");
    tokio::time::pause();
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(31)).await;
    drop(held);
    let (decision, probe_id) = client
        .circuits
        .before_public_attempt_with_probe_with_token(peer_id, true)
        .await;
    assert_eq!(
        decision,
        circuit::MeshAttemptDecision::Probe,
        "an expired outcome must not strand the half-open slot"
    );
    client
        .circuits
        .release_public_half_open_probe(peer_id, probe_id.expect("new probe"))
        .await;
    server.abort();
    let _ = server.await;
}

async fn occupy_completion_window(client: &MeshAwareHttpClient) -> Arc<tokio::sync::Semaphore> {
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let (started_tx, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
    for index in 0..32 {
        let release = release.clone();
        let started_tx = started_tx.clone();
        client.dispatch_critical_completion(format!("occupied-{index}"), async move {
            started_tx.send(()).expect("start receiver");
            release.acquire().await.expect("test release").forget();
        });
    }
    for _ in 0..32 {
        started_rx.recv().await.expect("active completion started");
    }
    release
}

#[tokio::test(start_paused = true)]
async fn public_cleanup_expiry_includes_queue_wait() {
    let client = MeshAwareHttpClient::new(reqwest::Client::new());
    let peer_id = xp_test_fixtures::primary_node_id();
    let release = occupy_completion_window(&client).await;
    let held = client.circuits.hold_public_peers_for_test().await;
    client.defer_public_failure(peer_id, client.circuits.next_operation(), None);
    tokio::time::advance(Duration::from_secs(31)).await;
    let (drained_tx, drained_rx) = tokio::sync::oneshot::channel();
    client.dispatch_critical_completion("queue-fence", async move {
        let _ = drained_tx.send(());
    });
    release.add_permits(32);
    drained_rx.await.expect("queued work was admitted");
    drop(held);
    assert_eq!(
        client.circuits.public_state(peer_id).await,
        BreakerState::Closed,
        "cleanup expired in the queue must not update circuit state"
    );
}

#[tokio::test(start_paused = true)]
async fn expired_public_lock_waits_release_the_active_window() {
    let client = MeshAwareHttpClient::new(reqwest::Client::new());
    let peer_id = xp_test_fixtures::primary_node_id();
    client.circuits.record_public_failure(peer_id).await;
    let held = client.circuits.hold_public_peers_for_test().await;
    for _ in 0..32 {
        client.defer_public_success(peer_id, client.circuits.next_operation(), None);
    }
    tokio::task::yield_now().await;
    let (started_tx, mut started_rx) = tokio::sync::oneshot::channel();
    client.dispatch_critical_completion("next-work", async move {
        let _ = started_tx.send(());
    });
    tokio::task::yield_now().await;
    assert!(started_rx.try_recv().is_err(), "all 32 slots are occupied");
    tokio::time::advance(Duration::from_secs(31)).await;
    tokio::time::timeout(Duration::from_secs(1), started_rx)
        .await
        .expect("expired lock waiters must free slots while the mutex remains held")
        .expect("successor completion started");
    drop(held);
    assert_eq!(
        client.circuits.public_state(peer_id).await,
        BreakerState::Open
    );
}

#[tokio::test]
async fn older_public_cleanup_cannot_displace_the_latest_overflow_outcome() {
    let client = MeshAwareHttpClient::new(reqwest::Client::new());
    let peer_id = xp_test_fixtures::primary_node_id();
    client.circuits.record_public_failure(peer_id).await;
    let release = occupy_completion_window(&client).await;
    for _ in 0..256 {
        let release = release.clone();
        client.dispatch_critical_completion("queued-work", async move {
            release
                .acquire()
                .await
                .expect("test queue release")
                .forget();
        });
    }
    let older = client.circuits.next_operation();
    let latest = client.circuits.next_operation();
    client.defer_public_success(peer_id, latest, None);
    client.defer_public_failure(peer_id, older, None);
    release.add_permits(32 + 256);
    tokio::time::timeout(Duration::from_secs(1), async {
        while client.circuits.public_state(peer_id).await != BreakerState::Closed {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("a delayed older failure must not replace the retained newer success");
}

#[tokio::test]
async fn newer_public_body_outcomes_survive_overflow_and_late_old_bodies() {
    for newest_succeeds in [true, false] {
        let client = MeshAwareHttpClient::new(reqwest::Client::new());
        let peer = super::peer_target_tests::primary_reverse_target(
            None,
            xp_test_fixtures::primary_api_url().to_owned(),
        );
        if newest_succeeds {
            client.circuits.record_public_failure(&peer.node_id).await;
        }
        let release = occupy_completion_window(&client).await;
        for _ in 0..256 {
            let release = release.clone();
            client.dispatch_critical_completion("queued-body-work", async move {
                release
                    .acquire()
                    .await
                    .expect("test queue release")
                    .forget();
            });
        }
        let older = client.circuits.next_operation();
        let newest = client.circuits.next_operation();
        for (operation_id, succeeds) in [(newest, newest_succeeds), (older, !newest_succeeds)] {
            let body = if succeeds {
                reqwest::Body::from(Vec::<u8>::new())
            } else {
                reqwest::Body::wrap_stream(futures_util::stream::once(async {
                    Err::<bytes::Bytes, _>(std::io::Error::other("upstream body failed"))
                }))
            };
            let deadline = Instant::now() + Duration::from_secs(5);
            let response = super::reverse::attach_response_with_finish(
                reqwest::Response::from(axum::http::Response::new(body)),
                deadline,
                Some(client.public_success_telemetry_callback(
                    &peer,
                    Instant::now(),
                    false,
                    false,
                    0,
                    operation_id,
                    None,
                    deadline,
                )),
            );
            assert_eq!(response.bytes().await.is_ok(), succeeds);
        }
        release.add_permits(32 + 256);
        let expected = if newest_succeeds {
            BreakerState::Closed
        } else {
            BreakerState::Open
        };
        tokio::time::timeout(Duration::from_secs(1), async {
            while client.circuits.public_state(&peer.node_id).await != expected {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("a later old body must not displace the newer signed outcome");
    }
}

#[tokio::test]
async fn repeated_public_cleanup_stays_bounded_and_recovers_after_contention() {
    let client = MeshAwareHttpClient::new(reqwest::Client::new());
    let peer_id = xp_test_fixtures::primary_node_id();
    let metrics = tokio::runtime::Handle::current().metrics();
    let initial_tasks = metrics.num_alive_tasks();
    let held = client.circuits.hold_public_peers_for_test().await;
    for index in 0..512 {
        let operation_id = client.circuits.next_operation();
        if index % 2 == 0 {
            client.defer_public_success(peer_id, operation_id, None);
        } else {
            client.defer_public_failure(peer_id, operation_id, None);
        }
    }
    tokio::task::yield_now().await;
    let waiting_tasks = metrics.num_alive_tasks();
    drop(held);
    assert!(
        waiting_tasks <= initial_tasks + 33,
        "Public cleanup must share the 32-active window: {waiting_tasks} tasks"
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        while client.circuits.public_state(peer_id).await != BreakerState::Open {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the latest Public failure must converge after contention clears");
}
