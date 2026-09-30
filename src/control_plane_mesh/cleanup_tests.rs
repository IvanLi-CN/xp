use super::*;
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
async fn protocol_failure_cleanup_waits_through_same_epoch_barrier_write() {
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
    client.spawn_protocol_failure_cleanup(&peer, 0, None);
    tokio::time::sleep(Duration::from_millis(10)).await;
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
    .expect("cleanup should wait for a same-epoch barrier write");
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
