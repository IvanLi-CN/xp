use super::*;
use crate::reconcile::ReconcileHandle;
use futures_util::StreamExt;
use tokio::sync::oneshot;

#[tokio::test]
async fn mesh_body_success_callbacks_share_one_completion_worker() {
    let client = MeshAwareHttpClient::new(reqwest::Client::new());
    let peer = MeshPeerTarget {
        node_id: xp_test_fixtures::primary_node_id().to_owned(),
        node_name: xp_test_fixtures::primary_node_name().to_owned(),
        mesh_base_url: Some(xp_test_fixtures::primary_api_url().to_owned()),
        endpoint_transport: Some("xhttp_reality_fallback"),
        endpoint_fingerprint: Some("fingerprint".to_owned()),
        mesh_reason: MeshPeerReason::MeshAvailable,
        public_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
    };
    let request = MeshRequest {
        method: reqwest::Method::GET,
        path_and_query: "/api/admin/_internal/mesh/health".to_owned(),
        content_type: None,
        body: Vec::new(),
        total_budget: Duration::from_secs(1),
        allow_ambiguous_fallback: false,
        request_id: "mesh-completion-dispatcher-regression".to_owned(),
        route: InternalRoute::HealthV2,
        cluster_id: xp_test_fixtures::primary_cluster_id().to_owned(),
        sender_id: "sender".to_owned(),
        updates_active_path: false,
    };
    let transport = MeshTransportObservation {
        protocol: MeshTransportProtocol::H2,
        fingerprint: None,
    };

    for _ in 0..8 {
        let response = reqwest::Response::from(
            axum::http::Response::builder()
                .status(reqwest::StatusCode::OK)
                .body(reqwest::Body::from(Vec::<u8>::new()))
                .expect("synthetic response"),
        );
        let response = super::reverse::attach_response_with_finish(
            response,
            Instant::now() + Duration::from_secs(1),
            Some(client.mesh_success_telemetry_callback(
                &peer,
                Instant::now(),
                &request,
                transport,
                0,
                None,
                client.circuits.next_operation(),
                None,
                Instant::now() + Duration::from_secs(1),
            )),
        );
        let mut body = response.bytes_stream();
        assert!(body.next().await.is_none());
    }

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if client.completion_worker_starts_for_test() == Some(1) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Mesh body completions should share one worker");
}

#[tokio::test]
async fn mesh_success_telemetry_does_not_requeue_gate_reader() {
    let temp = tempfile::tempdir().expect("telemetry directory");
    let telemetry = MeshTelemetryHandle::load(temp.path()).expect("telemetry");
    let telemetry_state = telemetry.clone().hold_state_for_test().await;
    let gate_lock = Arc::new(tokio::sync::RwLock::new(()));
    let client = MeshAwareHttpClient::new(reqwest::Client::new())
        .with_mesh_observability(telemetry.clone())
        .with_mesh_gate_lock(gate_lock.clone());
    let peer = MeshPeerTarget {
        node_id: xp_test_fixtures::primary_node_id().to_owned(),
        node_name: xp_test_fixtures::primary_node_name().to_owned(),
        mesh_base_url: Some(xp_test_fixtures::primary_api_url().to_owned()),
        endpoint_transport: Some("xhttp_reality_fallback"),
        endpoint_fingerprint: Some("fingerprint".to_owned()),
        mesh_reason: MeshPeerReason::MeshAvailable,
        public_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
    };
    let request = MeshRequest {
        method: reqwest::Method::GET,
        path_and_query: "/api/admin/_internal/mesh/health".to_owned(),
        content_type: None,
        body: Vec::new(),
        total_budget: Duration::from_secs(1),
        allow_ambiguous_fallback: false,
        request_id: "telemetry-reader-regression".to_owned(),
        route: InternalRoute::HealthV2,
        cluster_id: xp_test_fixtures::primary_cluster_id().to_owned(),
        sender_id: "sender".to_owned(),
        updates_active_path: false,
    };
    let in_flight = gate_lock.clone().read_owned().await;
    let (started_tx, started_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let writer_lock = gate_lock.clone();
    let writer = tokio::spawn(async move {
        let _ = started_tx.send(());
        let _guard = writer_lock.write_owned().await;
        let _ = release_rx.await;
    });
    started_rx.await.expect("writer should start");
    tokio::task::yield_now().await;
    tokio::time::timeout(
        Duration::from_millis(100),
        client.record_mesh_success_after_body(
            &peer,
            Instant::now(),
            request.updates_active_path,
            MeshTransportObservation {
                protocol: MeshTransportProtocol::H2,
                fingerprint: None,
            },
            0,
            None,
            client.circuits.next_operation(),
            None,
            None,
            Instant::now() + Duration::from_millis(10),
        ),
    )
    .await
    .expect("telemetry must not wait for the held telemetry or gate locks");
    drop(telemetry_state);
    drop(in_flight);
    let _ = release_tx.send(());
    writer.await.expect("writer should finish");
}

#[tokio::test]
async fn snapshot_reservation_blocks_body_completion_epoch_readers() {
    let reconcile = ReconcileHandle::noop();
    assert!(
        reconcile
            .initialize_mesh_gate_until(true, Instant::now() + Duration::from_secs(1))
            .await
    );
    let telemetry_dir = tempfile::tempdir().expect("telemetry directory");
    let telemetry = MeshTelemetryHandle::load(telemetry_dir.path()).expect("telemetry");
    let telemetry_state = telemetry.clone().hold_state_for_test().await;
    let client = MeshAwareHttpClient::new(reqwest::Client::new())
        .with_mesh_observability(telemetry)
        .with_mesh_gate_epoch(reconcile.mesh_gate(), reconcile.mesh_gate_epoch())
        .with_mesh_epoch_barrier(reconcile.mesh_epoch_barrier())
        .with_snapshot_install_reservation(reconcile.snapshot_installing());
    let reservation = reconcile
        .begin_snapshot_install_until(Instant::now() + Duration::from_secs(1))
        .await
        .expect("snapshot should reserve Mesh admission");
    let peer = MeshPeerTarget {
        node_id: xp_test_fixtures::primary_node_id().to_owned(),
        node_name: xp_test_fixtures::primary_node_name().to_owned(),
        mesh_base_url: Some(xp_test_fixtures::primary_api_url().to_owned()),
        endpoint_transport: Some("xhttp_reality_fallback"),
        endpoint_fingerprint: Some("fingerprint".to_owned()),
        mesh_reason: MeshPeerReason::MeshAvailable,
        public_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
    };
    let request = MeshRequest {
        method: reqwest::Method::GET,
        path_and_query: "/api/admin/_internal/mesh/health".to_owned(),
        content_type: None,
        body: Vec::new(),
        total_budget: Duration::from_secs(2),
        allow_ambiguous_fallback: false,
        request_id: "snapshot-reservation-body-completion".to_owned(),
        route: InternalRoute::HealthV2,
        cluster_id: xp_test_fixtures::primary_cluster_id().to_owned(),
        sender_id: "sender".to_owned(),
        updates_active_path: false,
    };
    let (finished_tx, finished_rx) = oneshot::channel();
    let callback = client.mesh_success_telemetry_callback_with_completion(
        &peer,
        Instant::now(),
        &request,
        MeshTransportObservation {
            protocol: MeshTransportProtocol::H2,
            fingerprint: None,
        },
        reconcile.mesh_gate_epoch().load(Ordering::Acquire),
        None,
        client.circuits.next_operation(),
        None,
        Instant::now() + Duration::from_secs(2),
        finished_tx,
    );

    callback(crate::mesh_gate_body::BodyFinish::Complete);
    tokio::time::timeout(Duration::from_millis(500), finished_rx)
        .await
        .expect("body completion must not take an epoch reader during snapshot reservation")
        .expect("body completion signal");
    assert!(
        reconcile
            .hold_mesh_gate_until_raft_state_until(Instant::now() + Duration::from_millis(500))
            .await,
        "snapshot installation barrier must remain acquirable while deferred telemetry is blocked"
    );

    drop(reservation);
    drop(telemetry_state);
}
