use super::*;

#[tokio::test]
async fn public_signed_body_commits_success_only_after_completion() {
    use futures_util::StreamExt;

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
    let circuits = client.circuits();
    circuits
        .record_public_failure(xp_test_fixtures::primary_node_id())
        .await;
    circuits
        .set_public_probe_ready_for_test(xp_test_fixtures::primary_node_id())
        .await;
    let (decision, probe_id) = circuits
        .before_public_attempt_with_probe_with_token(xp_test_fixtures::primary_node_id(), true)
        .await;
    let probe_guard = client
        .public_probe_guard(xp_test_fixtures::primary_node_id(), decision, probe_id)
        .expect("public probe guard");
    let response = reqwest::Response::from(
        axum::http::Response::builder()
            .status(reqwest::StatusCode::OK)
            .body(reqwest::Body::from("response-body"))
            .expect("synthetic response"),
    );
    let response = super::reverse::attach_response_with_finish(
        response,
        Instant::now() + Duration::from_secs(1),
        Some(client.public_success_telemetry_callback(
            &peer,
            Instant::now(),
            true,
            false,
            0,
            circuits.next_operation(),
            Some(probe_guard),
            Instant::now() + Duration::from_secs(1),
        )),
    );
    let body = response.bytes().await.expect("response body");
    assert_eq!(body.as_ref(), b"response-body");
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if circuits
                .public_state(xp_test_fixtures::primary_node_id())
                .await
                == BreakerState::Closed
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("public success must commit after body completion");

    let error_client = MeshAwareHttpClient::new(reqwest::Client::new());
    let error_circuits = error_client.circuits();
    error_circuits
        .record_public_failure(xp_test_fixtures::primary_node_id())
        .await;
    error_circuits
        .set_public_probe_ready_for_test(xp_test_fixtures::primary_node_id())
        .await;
    let (decision, probe_id) = error_circuits
        .before_public_attempt_with_probe_with_token(xp_test_fixtures::primary_node_id(), true)
        .await;
    let probe_guard = error_client
        .public_probe_guard(xp_test_fixtures::primary_node_id(), decision, probe_id)
        .expect("public error probe guard");
    let response = reqwest::Response::from(
        axum::http::Response::builder()
            .status(reqwest::StatusCode::OK)
            .body(reqwest::Body::wrap_stream(futures_util::stream::once(
                async { Err::<Vec<u8>, _>(std::io::Error::other("synthetic body error")) },
            )))
            .expect("synthetic error response"),
    );
    let response = super::reverse::attach_response_with_finish(
        response,
        Instant::now() + Duration::from_secs(1),
        Some(error_client.public_success_telemetry_callback(
            &peer,
            Instant::now(),
            true,
            false,
            0,
            error_circuits.next_operation(),
            Some(probe_guard),
            Instant::now() + Duration::from_secs(1),
        )),
    );
    let mut body = response.bytes_stream();
    assert!(body.next().await.expect("body error item").is_err());
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if error_circuits
                .public_state(xp_test_fixtures::primary_node_id())
                .await
                == BreakerState::Open
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("body error must update the public failure state");
}
