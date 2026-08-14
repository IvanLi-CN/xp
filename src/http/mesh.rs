use super::*;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize)]
struct AdminMeshStatusResponse {
    generated_at: String,
    revision: u64,
    local: AdminMeshLocalStatus,
    peers: Vec<AdminMeshPeerStatus>,
    events: Vec<crate::mesh_telemetry::MeshTelemetryEvent>,
}

#[derive(Debug, Clone, Serialize)]
struct AdminMeshLocalStatus {
    node_id: String,
    node_name: String,
    cluster_id: String,
    role: String,
    leader_api_base_url: String,
    term: u64,
    canary: crate::vless_https_canary::VlessHttpsCanaryStatus,
}

#[derive(Debug, Clone, Serialize)]
struct AdminMeshPeerStatus {
    node_id: String,
    node_name: String,
    api_base_url: String,
    mesh_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mesh_capability: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mesh_reason: Option<crate::mesh_telemetry::MeshPeerReason>,
    current_path: Option<TelemetryPath>,
    quality: MeshQuality,
    stale: bool,
    breaker: BreakerState,
    last_sample_at: Option<String>,
    last_transition_at: Option<String>,
    availability_1h: Option<f64>,
    availability_24h: Option<f64>,
    mesh_availability_24h: Option<f64>,
    latency_p50_ms: Option<u32>,
    latency_p95_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mesh_transport: Option<AdminMeshTransportStatus>,
    buckets: Vec<crate::mesh_telemetry::MeshTelemetryBucket>,
}

#[derive(Debug, Clone, Serialize)]
struct AdminMeshTransportStatus {
    protocol: Option<MeshTransportProtocol>,
    health: MeshTransportHealth,
    connection_generation: u64,
    current_connection_requests: u64,
    requests_5m: u32,
    connection_starts_5m: u32,
    requests_1h: u32,
    connection_starts_1h: u32,
    last_connection_started_at: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(super) struct AdminMeshProbeRequest {
    #[serde(default)]
    node_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct AdminMeshProbeResponse {
    accepted_node_ids: Vec<String>,
    revision: u64,
}

pub(super) fn spawn_mesh_probe_worker(state: AppState) {
    tokio::spawn(async move {
        // Probe evidence is local operational telemetry, not replicated cluster state.
        let jitter_secs = u64::from(rand::random::<u8>() % 10);
        tokio::time::sleep(Duration::from_secs(jitter_secs)).await;
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        let mut probe_round = 0_u64;
        loop {
            interval.tick().await;
            probe_round = probe_round.wrapping_add(1);
            let probe_public_standby = probe_round % 5 == 1;
            let node_ids = {
                let store = state.store.lock().await;
                store
                    .list_nodes()
                    .into_iter()
                    .filter(|node| node.node_id != state.cluster.node_id)
                    .map(|node| node.node_id)
                    .collect::<Vec<_>>()
            };
            let probe_gate = state.mesh_telemetry.probe_gate();
            stream::iter(node_ids)
                .map(|node_id| {
                    let state = state.clone();
                    let probe_gate = probe_gate.clone();
                    async move {
                        let Ok(_permit) = probe_gate.acquire_owned().await else {
                            return;
                        };
                        if let Err(error) = probe_mesh_peer(&state, &node_id).await {
                            tracing::debug!(
                                peer_id = %node_id,
                                ?error,
                                "scheduled mesh probe failed"
                            );
                        }
                        if probe_public_standby
                            && let Err(error) = probe_mesh_public_standby(&state, &node_id).await
                        {
                            tracing::debug!(
                                peer_id = %node_id,
                                ?error,
                                "scheduled public standby probe failed"
                            );
                        }
                    }
                })
                .buffer_unordered(4)
                .collect::<Vec<_>>()
                .await;
        }
    });
}

pub(super) async fn admin_internal_raft_client_write(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(cmd): ApiJson<DesiredStateCommand>,
) -> Result<Json<RaftClientResponse>, ApiError> {
    let Some(Extension(internal)) = internal else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    let idempotency_request = internal
        .verified
        .as_ref()
        .filter(|verified| verified.context.route == internal_auth::InternalRoute::MeshV2)
        .map(|verified| {
            (
                verified.context.request_id.clone(),
                IdempotencyRequest {
                    sender_id: verified.context.sender_id.clone(),
                    semantic_sha256: verified.idempotency_sha256.clone(),
                },
            )
        });
    if let Some((request_id, request)) = idempotency_request.as_ref() {
        match state
            .internal_idempotency
            .begin(request_id, request)
            .await
            .map_err(|error| {
                ApiError::internal(format!("read internal idempotency ledger: {error}"))
            })? {
            IdempotencyBegin::Existing(stored) => {
                let response = serde_json::from_value(stored.body).map_err(|error| {
                    ApiError::internal(format!("decode stored internal result: {error}"))
                })?;
                return Ok(Json(response));
            }
            IdempotencyBegin::InFlight => {
                return Err(ApiError::new(
                    "outcome_unknown",
                    StatusCode::CONFLICT,
                    "internal mutation pending",
                ));
            }
            IdempotencyBegin::Mismatch => {
                return Err(ApiError::new(
                    "idempotency_request_mismatch",
                    StatusCode::CONFLICT,
                    "internal request id is already bound to a different request",
                ));
            }
            IdempotencyBegin::Full => {
                return Err(ApiError::new(
                    "idempotency_ledger_full",
                    StatusCode::TOO_MANY_REQUESTS,
                    "internal idempotency ledger is full; retry after active records expire",
                ));
            }
            IdempotencyBegin::New => {}
        }
    }
    let resp = state
        .raft
        .client_write(cmd)
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;
    if let Some((request_id, request)) = idempotency_request {
        state
            .internal_idempotency
            .finish(
                &request_id,
                &request,
                StoredResult {
                    status: StatusCode::OK.as_u16(),
                    body: serde_json::to_value(&resp).map_err(|error| {
                        ApiError::internal(format!("encode internal idempotency result: {error}"))
                    })?,
                },
            )
            .await
            .map_err(|error| {
                ApiError::internal(format!("persist internal idempotency result: {error}"))
            })?;
    }
    Ok(Json(resp))
}

async fn build_admin_mesh_status_response(state: &AppState) -> AdminMeshStatusResponse {
    let now = Utc::now();
    let telemetry = state.mesh_telemetry.snapshot().await;
    let telemetry_by_peer = telemetry
        .peers
        .iter()
        .map(|peer| (peer.peer_id.as_str(), peer))
        .collect::<BTreeMap<_, _>>();
    let (nodes, endpoints) = {
        let store = state.store.lock().await;
        (store.list_nodes(), store.list_endpoints())
    };
    let peers = nodes
        .into_iter()
        .filter(|node| node.node_id != state.cluster.node_id)
        .map(|node| {
            let target = crate::control_plane_mesh::peer_target_from_node(&node, &endpoints);
            let mesh_url = target.mesh_base_url.clone();
            let mesh_enabled = mesh_url.is_some();
            let peer = telemetry_by_peer.get(node.node_id.as_str()).copied();
            let mesh_reason = if mesh_enabled {
                Some(
                    peer.and_then(|peer| peer.last_mesh_reason)
                        .filter(|_| {
                            peer.and_then(|peer| peer.last_mesh_target.as_deref())
                                == target.mesh_base_url.as_deref()
                        })
                        .unwrap_or(crate::mesh_telemetry::MeshPeerReason::NoSample),
                )
            } else {
                Some(target.mesh_reason)
            };
            let (
                availability_1h,
                availability_24h,
                mesh_availability_24h,
                latency_p50_ms,
                latency_p95_ms,
            ) = peer.map_or((None, None, None, None, None), |peer| {
                let (p50, p95) = latency_percentiles_for(peer, 24 * 60, now);
                (
                    availability_for(peer, 60, now),
                    availability_for(peer, 24 * 60, now),
                    mesh_availability_for(peer, now),
                    p50,
                    p95,
                )
            });
            AdminMeshPeerStatus {
                node_id: node.node_id.clone(),
                node_name: node.node_name,
                api_base_url: node.api_base_url,
                mesh_url,
                mesh_capability: Some(
                    if mesh_enabled { "enabled" } else { "disabled" }.to_string(),
                ),
                mesh_reason,
                current_path: peer.and_then(|peer| peer.last_path),
                quality: peer.map_or(MeshQuality::Unknown, |peer| quality_for_peer(peer, now)),
                stale: is_mesh_peer_stale(peer, now),
                breaker: breaker_for_mesh_target(mesh_enabled, peer.and_then(|peer| peer.breaker)),
                last_sample_at: peer.and_then(|peer| peer.last_sample_at.clone()),
                last_transition_at: peer.and_then(|peer| peer.last_transition_at.clone()),
                availability_1h,
                availability_24h,
                mesh_availability_24h,
                latency_p50_ms,
                latency_p95_ms,
                mesh_transport: mesh_transport_status_for(mesh_enabled, peer, now),
                buckets: peer.map_or_else(Vec::new, |peer| {
                    crate::mesh_telemetry::buckets_for_last_24_hours(peer, now)
                }),
            }
        })
        .collect();
    let metrics = raft_metrics(state);
    AdminMeshStatusResponse {
        generated_at: telemetry.generated_at,
        revision: telemetry.revision,
        local: AdminMeshLocalStatus {
            node_id: state.cluster.node_id.clone(),
            node_name: state.cluster.node_name.clone(),
            cluster_id: state.cluster.cluster_id.clone(),
            role: if is_leader(&metrics) {
                "leader".to_string()
            } else {
                "follower".to_string()
            },
            leader_api_base_url: leader_api_base_url(&metrics).unwrap_or_default(),
            term: metrics.current_term,
            canary: crate::vless_https_canary::load_status(
                &state.config.data_dir,
                state.config.vless_canary_bind,
            ),
        },
        peers,
        events: telemetry.events,
    }
}

fn mesh_transport_status_for(
    mesh_enabled: bool,
    peer: Option<&crate::mesh_telemetry::MeshPeerTelemetry>,
    now: DateTime<Utc>,
) -> Option<AdminMeshTransportStatus> {
    if !mesh_enabled {
        return None;
    }
    let (requests_5m, connection_starts_5m) = peer
        .map(|peer| mesh_transport_counts_for(peer, 5, now))
        .unwrap_or_default();
    let (requests_1h, connection_starts_1h) = peer
        .map(|peer| mesh_transport_counts_for(peer, 60, now))
        .unwrap_or_default();
    Some(AdminMeshTransportStatus {
        protocol: peer.and_then(|peer| peer.last_mesh_protocol),
        health: mesh_transport_health_for(peer, now),
        connection_generation: peer.map_or(0, |peer| peer.connection_generation),
        current_connection_requests: peer.map_or(0, |peer| peer.current_connection_requests),
        requests_5m,
        connection_starts_5m,
        requests_1h,
        connection_starts_1h,
        last_connection_started_at: peer.and_then(|peer| peer.last_connection_started_at.clone()),
    })
}

fn breaker_for_mesh_target(mesh_enabled: bool, recorded: Option<BreakerState>) -> BreakerState {
    if mesh_enabled {
        recorded.unwrap_or(BreakerState::Closed)
    } else {
        BreakerState::Disabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_mesh_target_never_reports_an_active_breaker() {
        assert_eq!(
            breaker_for_mesh_target(false, Some(BreakerState::Open)),
            BreakerState::Disabled
        );
        assert_eq!(breaker_for_mesh_target(false, None), BreakerState::Disabled);
        assert_eq!(breaker_for_mesh_target(true, None), BreakerState::Closed);
    }

    #[test]
    fn mesh_availability_uses_only_the_last_24_hours() {
        let now = xp_test_fixtures::baseline_timestamp()
            .parse::<chrono::DateTime<Utc>>()
            .unwrap();
        let peer = crate::mesh_telemetry::MeshPeerTelemetry {
            buckets: std::collections::VecDeque::from([
                crate::mesh_telemetry::MeshTelemetryBucket {
                    minute: xp_test_fixtures::timestamp_at20231230_t230000_z().to_owned(),
                    mesh_success: 1,
                    ..Default::default()
                },
                crate::mesh_telemetry::MeshTelemetryBucket {
                    minute: xp_test_fixtures::baseline_timestamp().to_owned(),
                    mesh_failure: 1,
                    ..Default::default()
                },
            ]),
            ..Default::default()
        };

        assert_eq!(mesh_availability_for(&peer, now), Some(0.0));
    }

    #[test]
    fn mesh_transport_status_is_optional_and_preserves_unknown_state() {
        let now = Utc::now();
        assert!(mesh_transport_status_for(false, None, now).is_none());

        let unknown = mesh_transport_status_for(true, None, now).unwrap();
        assert_eq!(unknown.health, MeshTransportHealth::Unknown);
        assert_eq!(unknown.protocol, None);
        assert_eq!(unknown.connection_generation, 0);

        let peer = crate::mesh_telemetry::MeshPeerTelemetry {
            last_mesh_protocol: Some(MeshTransportProtocol::H2),
            connection_generation: 1,
            current_connection_requests: 12,
            buckets: std::collections::VecDeque::from([
                crate::mesh_telemetry::MeshTelemetryBucket {
                    minute: now.to_rfc3339(),
                    mesh_h2_requests: 12,
                    mesh_connection_starts: 1,
                    ..Default::default()
                },
            ]),
            ..Default::default()
        };
        let healthy = mesh_transport_status_for(true, Some(&peer), now).unwrap();
        assert_eq!(healthy.health, MeshTransportHealth::Healthy);
        assert_eq!(healthy.requests_5m, 12);
        assert_eq!(healthy.connection_starts_5m, 1);
    }

    #[test]
    fn mesh_status_etag_tracks_reuse_evidence_but_not_generation_time() {
        fn response(generated_at: &str, connection_starts_5m: u32) -> AdminMeshStatusResponse {
            AdminMeshStatusResponse {
                generated_at: generated_at.to_string(),
                revision: 7,
                local: AdminMeshLocalStatus {
                    node_id: "local".to_string(),
                    node_name: "local".to_string(),
                    cluster_id: "cluster".to_string(),
                    role: "leader".to_string(),
                    leader_api_base_url: "https://local.example.test".to_string(),
                    term: 3,
                    canary: crate::vless_https_canary::VlessHttpsCanaryStatus::disabled(
                        std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
                    ),
                },
                peers: vec![AdminMeshPeerStatus {
                    node_id: "peer".to_string(),
                    node_name: "peer".to_string(),
                    api_base_url: "https://peer.example.test".to_string(),
                    mesh_url: Some("https://peer.example.test:443".to_string()),
                    mesh_capability: Some("enabled".to_string()),
                    mesh_reason: Some(crate::mesh_telemetry::MeshPeerReason::MeshAvailable),
                    current_path: Some(TelemetryPath::Mesh),
                    quality: MeshQuality::Good,
                    stale: false,
                    breaker: BreakerState::Closed,
                    last_sample_at: None,
                    last_transition_at: None,
                    availability_1h: Some(1.0),
                    availability_24h: Some(1.0),
                    mesh_availability_24h: Some(1.0),
                    latency_p50_ms: Some(10),
                    latency_p95_ms: Some(20),
                    mesh_transport: Some(AdminMeshTransportStatus {
                        protocol: Some(MeshTransportProtocol::H2),
                        health: MeshTransportHealth::Healthy,
                        connection_generation: 2,
                        current_connection_requests: 12,
                        requests_5m: 12,
                        connection_starts_5m,
                        requests_1h: 60,
                        connection_starts_1h: 2,
                        last_connection_started_at: None,
                    }),
                    buckets: Vec::new(),
                }],
                events: Vec::new(),
            }
        }

        let first = response("2026-08-08T10:00:00Z", 1);
        let generated_later = response("2026-08-08T10:01:00Z", 1);
        let churning = response("2026-08-08T10:01:00Z", 3);

        assert_eq!(mesh_status_etag(&first), mesh_status_etag(&generated_later));
        assert_ne!(mesh_status_etag(&first), mesh_status_etag(&churning));
    }
}

fn mesh_availability_for(
    peer: &crate::mesh_telemetry::MeshPeerTelemetry,
    now: DateTime<Utc>,
) -> Option<f64> {
    let from = now - chrono::Duration::hours(24);
    let (success, total) = peer
        .buckets
        .iter()
        .filter_map(|bucket| {
            DateTime::parse_from_rfc3339(&bucket.minute)
                .ok()
                .map(|at| (bucket, at))
        })
        .filter(|(_, at)| at.with_timezone(&Utc) >= from)
        .fold((0u32, 0u32), |(success, total), (bucket, _)| {
            (
                success + bucket.mesh_success,
                total + bucket.mesh_success + bucket.mesh_failure,
            )
        });
    (total > 0).then_some(success as f64 / total as f64)
}

fn is_mesh_peer_stale(
    peer: Option<&crate::mesh_telemetry::MeshPeerTelemetry>,
    now: DateTime<Utc>,
) -> bool {
    peer.and_then(|peer| peer.last_sample_at.as_deref())
        .and_then(|sample| DateTime::parse_from_rfc3339(sample).ok())
        .is_some_and(|sample| {
            now.signed_duration_since(sample.with_timezone(&Utc)) > chrono::Duration::minutes(3)
        })
}

pub(super) async fn admin_get_mesh_status(
    Extension(state): Extension<AppState>,
    headers: HeaderMap,
) -> Response {
    let snapshot = build_admin_mesh_status_response(&state).await;
    let etag = mesh_status_etag(&snapshot);
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == etag)
    {
        return (StatusCode::NOT_MODIFIED, [(header::ETAG, etag)]).into_response();
    }
    (StatusCode::OK, [(header::ETAG, etag)], Json(snapshot)).into_response()
}

fn mesh_status_etag(snapshot: &AdminMeshStatusResponse) -> String {
    let mut stable_snapshot = snapshot.clone();
    stable_snapshot.generated_at.clear();
    let stable_bytes = serde_json::to_vec(&stable_snapshot).expect("serialize mesh status ETag");
    format!("\"mesh-{}\"", hex::encode(Sha256::digest(stable_bytes)))
}

pub(super) async fn admin_run_mesh_probes(
    Extension(state): Extension<AppState>,
    ApiJson(request): ApiJson<AdminMeshProbeRequest>,
) -> Result<Json<AdminMeshProbeResponse>, ApiError> {
    let nodes = {
        let store = state.store.lock().await;
        store.list_nodes()
    };
    let accepted_node_ids = if request.node_ids.is_empty() {
        nodes
            .into_iter()
            .filter(|node| node.node_id != state.cluster.node_id)
            .map(|node| node.node_id)
            .collect::<Vec<_>>()
    } else {
        if request.node_ids.len() > 50 {
            return Err(ApiError::invalid_request(
                "at most 50 mesh probe targets are allowed",
            ));
        }
        let known = nodes
            .into_iter()
            .map(|node| node.node_id)
            .collect::<BTreeSet<_>>();
        if request.node_ids.iter().collect::<BTreeSet<_>>().len() != request.node_ids.len()
            || request
                .node_ids
                .iter()
                .any(|id| !known.contains(id) || id == &state.cluster.node_id)
        {
            return Err(ApiError::invalid_request(
                "mesh probe contains an unknown or local node",
            ));
        }
        request.node_ids
    };
    for node_id in &accepted_node_ids {
        state
            .mesh_telemetry
            .record_event(
                node_id,
                "probe_requested",
                "operator requested an immediate mesh probe",
            )
            .await
            .map_err(|error| ApiError::internal(format!("persist mesh probe request: {error}")))?;
    }
    let state_for_probes = state.clone();
    let probe_node_ids = accepted_node_ids.clone();
    let probe_gate = state.mesh_telemetry.probe_gate();
    tokio::spawn(async move {
        stream::iter(probe_node_ids)
            .map(|node_id| {
                let state = state_for_probes.clone();
                let probe_gate = probe_gate.clone();
                async move {
                    let Ok(_permit) = probe_gate.acquire_owned().await else {
                        return;
                    };
                    if let Err(error) = probe_mesh_peer(&state, &node_id).await {
                        tracing::debug!(
                            peer_id = %node_id,
                            ?error,
                            "immediate mesh probe did not complete"
                        );
                    }
                }
            })
            .buffer_unordered(4)
            .collect::<Vec<_>>()
            .await;
    });
    let revision = state.mesh_telemetry.snapshot().await.revision;
    Ok(Json(AdminMeshProbeResponse {
        accepted_node_ids,
        revision,
    }))
}

async fn mesh_peer_target(state: &AppState, node_id: &str) -> Result<MeshPeerTarget, ApiError> {
    let (node, endpoints) = {
        let store = state.store.lock().await;
        (
            store.get_node(node_id),
            store
                .list_endpoints()
                .into_iter()
                .filter(|endpoint| endpoint.node_id == node_id)
                .collect::<Vec<_>>(),
        )
    };
    let node = node.ok_or_else(|| ApiError::not_found(format!("node not found: {node_id}")))?;
    Ok(crate::control_plane_mesh::peer_target_from_node(
        &node, &endpoints,
    ))
}

pub(super) async fn send_mesh_internal_read(
    state: &AppState,
    client: &MeshAwareHttpClient,
    node: &Node,
    path_and_query: String,
    budget: Duration,
) -> Result<reqwest::Response, ApiError> {
    send_mesh_internal_request(
        state,
        client,
        node,
        Method::GET,
        path_and_query,
        Vec::new(),
        None,
        budget,
        true,
        crate::id::new_ulid_string(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn send_mesh_internal_request(
    state: &AppState,
    client: &MeshAwareHttpClient,
    node: &Node,
    method: Method,
    path_and_query: String,
    body: Vec<u8>,
    content_type: Option<String>,
    budget: Duration,
    allow_ambiguous_fallback: bool,
    request_id: String,
) -> Result<reqwest::Response, ApiError> {
    let ca_key_pem = state
        .cluster_ca_key_pem
        .as_deref()
        .ok_or_else(|| ApiError::internal("cluster CA key is not available"))?;
    let peer = mesh_peer_target(state, &node.node_id).await?;
    client
        .send_peer_request(
            &peer,
            MeshRequest {
                method,
                path_and_query,
                content_type,
                body,
                total_budget: budget,
                allow_ambiguous_fallback,
                request_id,
                route: internal_auth::InternalRoute::MeshV2,
                cluster_id: state.cluster.cluster_id.clone(),
                sender_id: state.cluster.node_id.clone(),
                updates_active_path: true,
            },
            ca_key_pem,
            &state.cluster_ca_pem,
        )
        .await
        .map_err(|error| ApiError::gateway_timeout(error.to_string()))
}

pub(super) async fn probe_mesh_peer(state: &AppState, node_id: &str) -> Result<(), ApiError> {
    run_mesh_health_probe(state, node_id, false).await
}

pub(super) async fn probe_mesh_public_standby(
    state: &AppState,
    node_id: &str,
) -> Result<(), ApiError> {
    run_mesh_health_probe(state, node_id, true).await
}

async fn run_mesh_health_probe(
    state: &AppState,
    node_id: &str,
    public_only: bool,
) -> Result<(), ApiError> {
    let Some(ca_key_pem) = state.cluster_ca_key_pem.as_deref() else {
        return Err(ApiError::internal("cluster CA key is not available"));
    };
    let mut peer = mesh_peer_target(state, node_id).await?;
    if public_only {
        peer.mesh_base_url = None;
    }
    let client = state.mesh_client.clone();
    client
        .send_peer_request(
            &peer,
            MeshRequest {
                method: Method::GET,
                path_and_query: "/api/admin/_internal/mesh/health".to_string(),
                content_type: None,
                body: Vec::new(),
                total_budget: Duration::from_secs(5),
                allow_ambiguous_fallback: true,
                request_id: crate::id::new_ulid_string(),
                route: internal_auth::InternalRoute::HealthV2,
                cluster_id: state.cluster.cluster_id.clone(),
                sender_id: state.cluster.node_id.clone(),
                updates_active_path: !public_only,
            },
            ca_key_pem,
            &state.cluster_ca_pem,
        )
        .await
        .map_err(|error| ApiError::gateway_timeout(error.to_string()))?;
    Ok(())
}
