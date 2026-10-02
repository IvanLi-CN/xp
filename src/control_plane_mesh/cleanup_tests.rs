use super::peer_target_tests::primary_reverse_target;
use super::*;
use crate::reconcile::ReconcileHandle;
use std::sync::atomic::Ordering;

impl MeshAwareHttpClient {
    pub(crate) fn mesh_gate_lock_for_test(&self) -> Arc<tokio::sync::RwLock<()>> {
        self.mesh_gate_lock.clone()
    }

    pub(crate) async fn hold_direct_validation_records_for_test(
        &self,
    ) -> tokio::sync::OwnedMutexGuard<BTreeMap<String, circuit::DirectValidationRecord>> {
        self.direct_validation.hold_records_for_test().await
    }
}

#[tokio::test]
async fn protocol_failure_cleanup_waits_for_epoch_barrier_before_state_update() {
    let client = MeshAwareHttpClient::new(reqwest::Client::new()).with_direct_validation_required();
    let peer = MeshPeerTarget {
        node_id: "peer".to_owned(),
        node_name: "peer".to_owned(),
        mesh_base_url: Some("https://mesh.example".to_owned()),
        endpoint_transport: Some("vision_tcp"),
        endpoint_fingerprint: Some("fingerprint".to_owned()),
        mesh_reason: MeshPeerReason::MeshAvailable,
        public_base_url: "https://public.example".to_owned(),
    };
    let barrier_writer = client.mesh_epoch_barrier.clone().write_owned().await;
    client.spawn_protocol_failure_cleanup(&peer, 0, None, client.circuits.next_operation());
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        client.circuits().state("peer", true).await,
        BreakerState::Closed
    );
    drop(barrier_writer);

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if client
                .circuits()
                .before_attempt_with_probe("peer", true, false)
                .await
                == MeshAttemptDecision::Quarantined
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cleanup should quarantine the peer");
}

#[tokio::test]
async fn stale_failure_cleanup_cannot_overwrite_newer_success() {
    let client = MeshAwareHttpClient::new(reqwest::Client::new()).with_direct_validation_required();
    let peer = MeshPeerTarget {
        node_id: "peer".to_owned(),
        node_name: "peer".to_owned(),
        mesh_base_url: Some("https://mesh.example".to_owned()),
        endpoint_transport: Some("vision_tcp"),
        endpoint_fingerprint: Some("fingerprint".to_owned()),
        mesh_reason: MeshPeerReason::MeshAvailable,
        public_base_url: "https://public.example".to_owned(),
    };
    let stale_operation = client.circuits.next_operation();
    let current_operation = client.circuits.next_operation();
    client
        .circuits
        .record_success_at(&peer.node_id, current_operation)
        .await;
    client
        .mark_direct_validation_success_with_operation(&peer, None, current_operation)
        .await;

    client.spawn_protocol_failure_cleanup(&peer, 0, None, stale_operation);
    tokio::time::sleep(Duration::from_millis(150)).await;

    assert_eq!(
        client.circuits().state("peer", true).await,
        BreakerState::Closed
    );
    assert_eq!(
        client.direct_validation_state_for(&peer).await,
        DirectValidationState::Verified
    );
}

#[tokio::test]
async fn protocol_validation_timeout_converges_in_background() {
    let client = MeshAwareHttpClient::new(reqwest::Client::new()).with_direct_validation_required();
    let peer = MeshPeerTarget {
        node_id: "peer".to_owned(),
        node_name: "peer".to_owned(),
        mesh_base_url: Some("https://mesh.example".to_owned()),
        endpoint_transport: Some("vision_tcp"),
        endpoint_fingerprint: Some("fingerprint".to_owned()),
        mesh_reason: MeshPeerReason::MeshAvailable,
        public_base_url: "https://public.example".to_owned(),
    };
    let records_lock = client.hold_direct_validation_records_for_test().await;
    let gate_guard = client.mesh_gate_lock.clone().read_owned().await;
    assert!(
        client
            .record_protocol_failure_for_epoch(
                &peer,
                0,
                None,
                &gate_guard,
                Instant::now() + Duration::from_millis(10),
            )
            .await
            .is_some()
    );
    drop(gate_guard);
    drop(records_lock);

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if client.direct_validation_state_for(&peer).await
                == DirectValidationState::ProtocolRejected
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("timed-out validation update should converge in the background");
}

#[tokio::test]
async fn transport_validation_timeout_converges_in_background() {
    let client = MeshAwareHttpClient::new(reqwest::Client::new()).with_direct_validation_required();
    let peer = MeshPeerTarget {
        node_id: "peer".to_owned(),
        node_name: "peer".to_owned(),
        mesh_base_url: Some("https://mesh.example".to_owned()),
        endpoint_transport: Some("vision_tcp"),
        endpoint_fingerprint: Some("fingerprint".to_owned()),
        mesh_reason: MeshPeerReason::MeshAvailable,
        public_base_url: "https://public.example".to_owned(),
    };
    client.mark_direct_validation_success_at(&peer, None).await;
    let records_lock = client.hold_direct_validation_records_for_test().await;
    let gate_guard = client.mesh_gate_lock.clone().read_owned().await;
    client
        .record_mesh_transport_failure(
            &peer,
            MeshPeerReason::TransportError,
            "synthetic timeout".to_owned(),
            0,
            gate_guard,
            None,
            Instant::now() + Duration::from_millis(10),
        )
        .await;
    drop(records_lock);

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if client.direct_validation_state_for(&peer).await
                == DirectValidationState::TransportFailed
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("timed-out transport validation update should converge in the background");
}

#[tokio::test]
async fn public_admission_timeout_does_not_dispatch_after_circuit_lock_wait() {
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let (public_base_url, public_requests, public_task) =
        super::peer_target_tests::spawn_signed_public(&ca.key_pem, &ca.cert_pem).await;
    let peer = super::peer_target_tests::primary_reverse_target(None, public_base_url);
    let client = MeshAwareHttpClient::new(reqwest::Client::new());
    let public_lock = client.circuits().hold_public_peers_for_test().await;

    let result = client
        .send_peer_request(
            &peer,
            MeshRequest {
                method: reqwest::Method::GET,
                path_and_query: "/api/admin/_internal/mesh/health".to_owned(),
                content_type: None,
                body: Vec::new(),
                total_budget: Duration::from_millis(50),
                allow_ambiguous_fallback: true,
                request_id: "public-admission-lock-timeout".to_owned(),
                route: InternalRoute::HealthV2,
                cluster_id: xp_test_fixtures::cluster_fixture53().to_owned(),
                sender_id: xp_test_fixtures::tertiary_node_id().to_owned(),
                updates_active_path: false,
            },
            &ca.key_pem,
            &ca.cert_pem,
        )
        .await
        .expect_err("Public admission must stop at the request deadline");

    assert!(matches!(result, MeshRequestError::PreDispatchTimeout));
    assert_eq!(public_requests.load(Ordering::SeqCst), 0);
    drop(public_lock);
    public_task.abort();
}

#[tokio::test]
async fn public_success_cleanup_converges_after_request_deadline() {
    let circuits = PeerCircuitBreakers::default();
    circuits.record_public_failure("peer").await;
    circuits.set_public_probe_ready_for_test("peer").await;
    let operation_id = circuits.next_operation();
    let public_peers = circuits.hold_public_peers_for_test().await;
    circuits.spawn_public_success_cleanup("peer", operation_id);
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(25), circuits.public_state("peer"))
            .await
            .is_err(),
        "public success cleanup should remain pending while its state lock is held"
    );
    drop(public_peers);

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if circuits.public_state("peer").await == BreakerState::Closed {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("unbounded public success cleanup should record the breaker state");
}

#[tokio::test]
async fn mesh_admission_timeout_does_not_public_dispatch_an_ordinary_mutation() {
    let reconcile = ReconcileHandle::noop();
    let gate = reconcile.mesh_gate();
    let epoch = reconcile.mesh_gate_epoch();
    let in_flight_mesh_read = reconcile.mesh_gate_lock().read_owned().await;
    let transition = tokio::spawn({
        let reconcile = reconcile.clone();
        async move { reconcile.initialize_mesh_gate(false).await }
    });
    tokio::task::yield_now().await;

    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let (public_base_url, public_requests, public_task) =
        super::peer_target_tests::spawn_signed_public(&ca.key_pem, &ca.cert_pem).await;
    let (mesh_base_url, mesh_requests, mesh_task) =
        super::peer_target_tests::spawn_stalling_mesh().await;
    let peer = primary_reverse_target(Some(mesh_base_url), public_base_url);
    let client =
        MeshAwareHttpClient::from_transport_clients(reqwest::Client::new(), reqwest::Client::new())
            .with_mesh_gate_epoch(gate, epoch)
            .with_mesh_gate_lock(reconcile.mesh_gate_lock());

    let result = tokio::time::timeout(
        Duration::from_millis(900),
        client.send_peer_request(
            &peer,
            MeshRequest {
                method: reqwest::Method::POST,
                path_and_query: "/api/admin/_internal/raft/client-write".to_owned(),
                content_type: Some("application/json".to_owned()),
                body: br#"{\"op\":\"set\"}"#.to_vec(),
                total_budget: Duration::from_secs(1),
                allow_ambiguous_fallback: false,
                request_id: "mesh-admission-timeout-no-mutation-fallback".to_owned(),
                route: InternalRoute::MeshV2,
                cluster_id: xp_test_fixtures::cluster_fixture53().to_owned(),
                sender_id: xp_test_fixtures::tertiary_node_id().to_owned(),
                updates_active_path: true,
            },
            &ca.key_pem,
            &ca.cert_pem,
        ),
    )
    .await
    .expect("ordinary mutation admission must remain bounded");
    assert!(matches!(result, Err(MeshRequestError::PreDispatchTimeout)));
    assert_eq!(mesh_requests.load(Ordering::SeqCst), 0);
    assert_eq!(public_requests.load(Ordering::SeqCst), 0);

    drop(in_flight_mesh_read);
    transition
        .await
        .expect("gate transition task should finish");
    mesh_task.abort();
    public_task.abort();
}

#[tokio::test]
async fn dispatched_mesh_timeout_is_not_reported_as_pre_dispatch_timeout() {
    let (mesh_base_url, mesh_requests, mesh_task) =
        super::peer_target_tests::spawn_stalling_mesh().await;
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let (public_base_url, public_requests, public_task) =
        super::peer_target_tests::spawn_signed_public(&ca.key_pem, &ca.cert_pem).await;
    let peer =
        super::peer_target_tests::primary_reverse_target(Some(mesh_base_url), public_base_url);
    let client =
        MeshAwareHttpClient::from_transport_clients(reqwest::Client::new(), reqwest::Client::new());

    let result = client
        .send_peer_request(
            &peer,
            MeshRequest {
                method: reqwest::Method::POST,
                path_and_query: "/api/admin/_internal/raft/client-write".to_owned(),
                content_type: Some("application/json".to_owned()),
                body: br#"{"op":"set"}"#.to_vec(),
                total_budget: Duration::from_secs(1),
                allow_ambiguous_fallback: false,
                request_id: "dispatched-mesh-timeout-classification".to_owned(),
                route: InternalRoute::MeshV2,
                cluster_id: xp_test_fixtures::cluster_fixture53().to_owned(),
                sender_id: xp_test_fixtures::tertiary_node_id().to_owned(),
                updates_active_path: true,
            },
            &ca.key_pem,
            &ca.cert_pem,
        )
        .await
        .expect_err("a dispatched mutation timeout must fail closed");

    assert!(
        matches!(
            result,
            MeshRequestError::TransportTimeout | MeshRequestError::OutcomeUnknown
        ),
        "dispatched Mesh timeout must retain an ambiguous outcome: {result:?}"
    );
    assert_eq!(mesh_requests.load(Ordering::SeqCst), 1);
    assert_eq!(public_requests.load(Ordering::SeqCst), 0);
    mesh_task.abort();
    public_task.abort();
}
