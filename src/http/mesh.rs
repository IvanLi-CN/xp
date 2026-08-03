use super::*;

#[derive(Debug, Serialize)]
struct AdminMeshStatusResponse {
    generated_at: String,
    revision: u64,
    local: AdminMeshLocalStatus,
    peers: Vec<AdminMeshPeerStatus>,
    events: Vec<crate::mesh_telemetry::MeshTelemetryEvent>,
}

#[derive(Debug, Serialize)]
struct AdminMeshLocalStatus {
    node_id: String,
    node_name: String,
    cluster_id: String,
    role: String,
    leader_api_base_url: String,
    term: u64,
    mesh_proxy_status: String,
    mesh_proxy_reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct AdminMeshPeerStatus {
    node_id: String,
    node_name: String,
    api_base_url: String,
    mesh_url: Option<String>,
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
    buckets: Vec<crate::mesh_telemetry::MeshTelemetryBucket>,
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
            stream::iter(node_ids)
                .map(|node_id| {
                    let state = state.clone();
                    async move {
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
    let request_id = internal
        .verified
        .as_ref()
        .filter(|verified| verified.context.route == internal_auth::InternalRoute::MeshV2)
        .map(|verified| verified.context.request_id.clone());
    if let Some(request_id) = request_id.as_deref() {
        match state
            .internal_idempotency
            .begin(request_id)
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
    if let Some(request_id) = request_id {
        state
            .internal_idempotency
            .finish(
                &request_id,
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
            let matching = endpoints
                .iter()
                .filter(|endpoint| endpoint.node_id == node.node_id)
                .filter(|endpoint| managed_default_vless_endpoint(endpoint).is_some())
                .collect::<Vec<_>>();
            let mesh_url = match matching.as_slice() {
                [endpoint] => Some(format!("https://{}:{}", node.access_host, endpoint.port)),
                _ => None,
            };
            let peer = telemetry_by_peer.get(node.node_id.as_str()).copied();
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
                    mesh_availability_for(peer),
                    p50,
                    p95,
                )
            });
            AdminMeshPeerStatus {
                node_id: node.node_id.clone(),
                node_name: node.node_name,
                api_base_url: node.api_base_url,
                mesh_url,
                current_path: peer.and_then(|peer| peer.last_path),
                quality: peer.map_or(MeshQuality::Unknown, |peer| quality_for_peer(peer, now)),
                stale: is_mesh_peer_stale(peer, now),
                breaker: peer.and_then(|peer| peer.breaker).unwrap_or({
                    if matching.len() == 1 {
                        BreakerState::Closed
                    } else {
                        BreakerState::Disabled
                    }
                }),
                last_sample_at: peer.and_then(|peer| peer.last_sample_at.clone()),
                last_transition_at: peer.and_then(|peer| peer.last_transition_at.clone()),
                availability_1h,
                availability_24h,
                mesh_availability_24h,
                latency_p50_ms,
                latency_p95_ms,
                buckets: peer.map_or_else(Vec::new, |peer| peer.buckets.iter().cloned().collect()),
            }
        })
        .collect();
    let proxy = state.mesh_proxy_state.snapshot().await;
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
            mesh_proxy_status: proxy.status.as_str().to_string(),
            mesh_proxy_reason: proxy.fallback_reason,
        },
        peers,
        events: telemetry.events,
    }
}

fn mesh_availability_for(peer: &crate::mesh_telemetry::MeshPeerTelemetry) -> Option<f64> {
    let (success, total) = peer
        .buckets
        .iter()
        .fold((0u32, 0u32), |(success, total), bucket| {
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
    let etag = format!("\"mesh-{}\"", snapshot.revision);
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == etag)
    {
        return (StatusCode::NOT_MODIFIED, [(header::ETAG, etag)]).into_response();
    }
    (StatusCode::OK, [(header::ETAG, etag)], Json(snapshot)).into_response()
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
        let state_for_probe = state.clone();
        let node_id = node_id.clone();
        tokio::spawn(async move {
            if let Err(error) = probe_mesh_peer(&state_for_probe, &node_id).await {
                tracing::debug!(
                    peer_id = %node_id,
                    ?error,
                    "immediate mesh probe did not complete"
                );
            }
        });
    }
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
    let managed = endpoints
        .iter()
        .filter(|endpoint| managed_default_vless_endpoint(endpoint).is_some())
        .collect::<Vec<_>>();
    let mesh_base_url = match managed.as_slice() {
        [endpoint] => Some(format!("https://{}:{}", node.access_host, endpoint.port)),
        _ => None,
    };
    Ok(MeshPeerTarget {
        node_id: node.node_id,
        node_name: node.node_name,
        mesh_base_url,
        public_base_url: node.api_base_url,
    })
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
    let client = build_cluster_http_client(state)?;
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
            },
            ca_key_pem,
            &state.cluster_ca_pem,
        )
        .await
        .map_err(|error| ApiError::gateway_timeout(error.to_string()))?;
    Ok(())
}
