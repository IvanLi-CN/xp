use super::*;

#[test]
fn mesh_proxy_status_strings_are_stable() {
    assert_eq!(MeshProxyStatus::Disabled.as_str(), "disabled");
    assert_eq!(MeshProxyStatus::Ready.as_str(), "ready");
    assert_eq!(MeshProxyStatus::Fallback.as_str(), "fallback");
    assert_eq!(MeshProxyStatus::Degraded.as_str(), "degraded");
}

#[test]
fn invalid_proxy_url_is_rejected() {
    let err = apply_optional_proxy(reqwest::Client::builder(), Some("not a url")).unwrap_err();
    assert!(err.to_string().contains("invalid proxy url"));
}

#[test]
fn mesh_budget_reserves_the_public_remainder() {
    assert_eq!(
        mesh_attempt_budget(Duration::from_millis(100)),
        Duration::from_millis(500)
    );
    assert_eq!(
        mesh_attempt_budget(Duration::from_secs(6)),
        Duration::from_secs(2)
    );
    assert_eq!(
        mesh_attempt_budget(Duration::from_secs(30)),
        Duration::from_secs(5)
    );
}

#[tokio::test]
async fn peer_breaker_opens_after_three_transport_failures() {
    let breakers = PeerCircuitBreakers::default();
    assert_eq!(
        breakers.before_attempt("peer-a", true).await,
        MeshAttemptDecision::Attempt
    );
    assert_eq!(
        breakers.record_retryable_failure("peer-a").await,
        BreakerState::Closed
    );
    assert_eq!(
        breakers.record_retryable_failure("peer-a").await,
        BreakerState::Closed
    );
    assert_eq!(
        breakers.record_retryable_failure("peer-a").await,
        BreakerState::Open
    );
    assert_eq!(
        breakers.before_attempt("peer-a", true).await,
        MeshAttemptDecision::SkipOpen
    );
    assert_eq!(
        breakers.record_success("peer-a").await,
        BreakerState::Closed
    );
}

#[tokio::test]
async fn half_open_allows_exactly_one_mesh_probe() {
    let breakers = PeerCircuitBreakers::default();
    breakers.peers.lock().await.insert(
        "peer-a".to_string(),
        PeerCircuit {
            failures: MESH_FAILURES_BEFORE_OPEN,
            open_count: 1,
            retry_at: Some(Instant::now() - Duration::from_secs(1)),
            half_open_in_flight: false,
        },
    );
    assert_eq!(
        breakers.before_attempt("peer-a", true).await,
        MeshAttemptDecision::Probe
    );
    assert_eq!(
        breakers.before_attempt("peer-a", true).await,
        MeshAttemptDecision::SkipOpen
    );
    assert_eq!(
        breakers.record_success("peer-a").await,
        BreakerState::Closed
    );
}

#[tokio::test]
async fn protocol_failure_releases_the_half_open_probe_slot() {
    let breakers = PeerCircuitBreakers::default();
    breakers.peers.lock().await.insert(
        "peer-a".to_string(),
        PeerCircuit {
            retry_at: Some(Instant::now() - Duration::from_secs(1)),
            ..PeerCircuit::default()
        },
    );
    assert_eq!(
        breakers.before_attempt("peer-a", true).await,
        MeshAttemptDecision::Probe
    );
    breakers.release_half_open_probe("peer-a").await;
    assert_eq!(
        breakers.before_attempt("peer-a", true).await,
        MeshAttemptDecision::Probe
    );
}
