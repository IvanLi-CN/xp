use std::collections::BTreeSet;
use std::sync::atomic::Ordering;

use futures_util::stream;
use reqwest::Method;

use super::*;
use crate::control_plane_mesh::{MeshRequest, PeerDirectPath};
use crate::mesh_telemetry::{DirectConnectivity, PublicFallbackHealth};

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct AdminMeshProbeRequest {
    #[serde(default)]
    node_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdminMeshProbeResponse {
    accepted_node_ids: Vec<String>,
    not_applicable_node_ids: Vec<String>,
    revision: u64,
}

pub(crate) fn spawn_mesh_probe_worker(state: AppState) {
    tokio::spawn(async move {
        // Probe evidence is local operational telemetry, not replicated cluster state.
        let jitter_secs = u64::from(rand::random::<u8>() % 10);
        tokio::time::sleep(Duration::from_secs(jitter_secs)).await;
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            let node_ids = mesh_peer_ids(&state).await;
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
                    }
                })
                .buffer_unordered(4)
                .collect::<Vec<_>>()
                .await;
        }
    });
}

pub(crate) fn spawn_mesh_public_probe_worker(state: AppState) {
    tokio::spawn(async move {
        let jitter_secs = u64::from(rand::random::<u8>() % 10);
        tokio::time::sleep(Duration::from_secs(jitter_secs)).await;
        let mut interval = tokio::time::interval(Duration::from_secs(300));
        loop {
            interval.tick().await;
            let node_ids = mesh_peer_ids(&state).await;
            let probe_gate = state.mesh_telemetry.probe_gate();
            stream::iter(node_ids)
                .map(|node_id| {
                    let state = state.clone();
                    let probe_gate = probe_gate.clone();
                    async move {
                        let Ok(_permit) = probe_gate.acquire_owned().await else {
                            return;
                        };
                        if let Err(error) = probe_mesh_public_standby(&state, &node_id).await {
                            tracing::debug!(
                                peer_id = %node_id,
                                ?error,
                                "scheduled public fallback probe failed"
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

async fn mesh_peer_ids(state: &AppState) -> Vec<String> {
    let store = state.store.lock().await;
    store
        .list_nodes()
        .into_iter()
        .filter(|node| node.node_id != state.cluster.node_id)
        .map(|node| node.node_id)
        .collect()
}

pub(crate) async fn admin_run_mesh_probes(
    Extension(state): Extension<AppState>,
    ApiJson(request): ApiJson<AdminMeshProbeRequest>,
) -> Result<Json<AdminMeshProbeResponse>, ApiError> {
    let nodes = {
        let store = state.store.lock().await;
        store.list_nodes()
    };
    let operator_probe_permit = state
        .mesh_telemetry
        .try_acquire_operator_probe_batch()
        .ok_or_else(|| ApiError::conflict("a mesh probe batch is already running"))?;
    let requested_node_ids = if request.node_ids.is_empty() {
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
    let mut accepted_node_ids = Vec::new();
    let mut not_applicable_node_ids = Vec::new();
    for node_id in requested_node_ids {
        let target = mesh_peer_target(&state, &node_id).await?;
        let gate = state.reconcile.mesh_gate().load(Ordering::Acquire);
        if gate && target.mesh_base_url.is_some() {
            accepted_node_ids.push(node_id);
            continue;
        }
        state
            .mesh_telemetry
            .set_direct_connectivity(
                &node_id,
                target.node_name.as_str(),
                DirectConnectivity::NotApplicable,
            )
            .await
            .map_err(|error| {
                ApiError::internal(format!("persist direct probe outcome: {error}"))
            })?;
        not_applicable_node_ids.push(node_id);
    }
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
        let _operator_probe_permit = operator_probe_permit;
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
        not_applicable_node_ids,
        revision,
    }))
}

pub(crate) async fn probe_mesh_peer(
    state: &AppState,
    node_id: &str,
) -> Result<DirectConnectivity, ApiError> {
    let target = mesh_peer_target(state, node_id).await?;
    let gate = state.reconcile.mesh_gate().load(Ordering::Acquire);
    let outcome = if !gate || target.mesh_base_url.is_none() {
        DirectConnectivity::NotApplicable
    } else {
        let Some(ca_key_pem) = state.cluster_ca_key_pem.as_deref() else {
            return Err(ApiError::internal("cluster CA key is not available"));
        };
        let request = MeshRequest {
            method: Method::GET,
            path_and_query: "/api/admin/_internal/mesh/health".to_string(),
            content_type: None,
            body: Vec::new(),
            total_budget: Duration::from_secs(5),
            allow_ambiguous_fallback: false,
            request_id: crate::id::new_ulid_string(),
            route: internal_auth::InternalRoute::HealthV2,
            cluster_id: state.cluster.cluster_id.clone(),
            sender_id: state.cluster.node_id.clone(),
            updates_active_path: false,
        };
        match state
            .mesh_client
            .send_peer_direct_request(
                &target,
                PeerDirectPath::RealityMesh,
                request,
                ca_key_pem,
                &state.cluster_ca_pem,
            )
            .await
        {
            Ok(response) => match preflight::consume_bounded_preflight_body(response).await {
                Ok(()) => DirectConnectivity::Verified,
                Err(preflight::MeshPreflightBodyError::Transport(message)) => {
                    tracing::debug!(
                        peer_id = %target.node_id,
                        error = %message,
                        "direct probe body failed"
                    );
                    DirectConnectivity::Unavailable
                }
                Err(preflight::MeshPreflightBodyError::Oversized) => {
                    tracing::debug!(peer_id = %target.node_id, "direct probe body oversized");
                    DirectConnectivity::Unavailable
                }
            },
            Err(error) => {
                tracing::debug!(peer_id = %target.node_id, ?error, "direct probe failed");
                DirectConnectivity::Unavailable
            }
        }
    };
    state
        .mesh_telemetry
        .set_direct_connectivity(&target.node_id, target.node_name.as_str(), outcome)
        .await
        .map_err(|error| ApiError::internal(format!("persist direct probe outcome: {error}")))?;
    Ok(outcome)
}

pub(crate) async fn probe_mesh_public_standby(
    state: &AppState,
    node_id: &str,
) -> Result<PublicFallbackHealth, ApiError> {
    let target = mesh_peer_target(state, node_id).await?;
    let Some(ca_key_pem) = state.cluster_ca_key_pem.as_deref() else {
        return Err(ApiError::internal("cluster CA key is not available"));
    };
    let request = MeshRequest {
        method: Method::GET,
        path_and_query: "/api/admin/_internal/mesh/health".to_string(),
        content_type: None,
        body: Vec::new(),
        total_budget: crate::control_plane_mesh::PUBLIC_HEALTH_PROBE_BUDGET,
        allow_ambiguous_fallback: false,
        request_id: crate::id::new_ulid_string(),
        route: internal_auth::InternalRoute::HealthV2,
        cluster_id: state.cluster.cluster_id.clone(),
        sender_id: state.cluster.node_id.clone(),
        updates_active_path: false,
    };
    let outcome = match state
        .mesh_client
        .send_public_probe(&target, &request, ca_key_pem, &state.cluster_ca_pem)
        .await
    {
        Ok(response) => match preflight::consume_bounded_preflight_body(response).await {
            Ok(()) => PublicFallbackHealth::Available,
            Err(preflight::MeshPreflightBodyError::Transport(message)) => {
                tracing::debug!(
                    peer_id = %target.node_id,
                    error = %message,
                    "public probe body failed"
                );
                PublicFallbackHealth::Unavailable
            }
            Err(preflight::MeshPreflightBodyError::Oversized) => {
                tracing::debug!(peer_id = %target.node_id, "public probe body oversized");
                PublicFallbackHealth::Unavailable
            }
        },
        Err(error) => {
            tracing::debug!(peer_id = %target.node_id, ?error, "public probe failed");
            PublicFallbackHealth::Unavailable
        }
    };
    state
        .mesh_telemetry
        .set_public_fallback_health(&target.node_id, target.node_name.as_str(), outcome)
        .await
        .map_err(|error| ApiError::internal(format!("persist public probe outcome: {error}")))?;
    Ok(outcome)
}
