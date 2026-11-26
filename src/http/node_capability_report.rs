//! Signed node self-report of release version and capabilities.
//!
//! The report is durable evidence used only when that node does not answer a capability probe at
//! all. It never substitutes for a live answer from a node that does respond, and it is rejected
//! unless the signed request came from the exact node it describes.

use std::time::Duration;

use axum::{Extension, Json, http::StatusCode};
use chrono::{SecondsFormat, Utc};
use serde::Deserialize;

use super::*;

const MAX_REPORTED_CAPABILITIES: usize = 256;
const MAX_CAPABILITY_LEN: usize = 128;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InternalReportNodeStateRequest {
    node_id: String,
    xp_version: String,
    capabilities: Vec<String>,
}

/// Accept a signed capability report only from its own subject and only for an existing member.
pub(super) async fn admin_internal_report_node_state(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<InternalReportNodeStateRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Some(Extension(internal)) = internal else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    let Some(verified) = internal.verified.as_ref() else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    // A node may only report about itself; the signature already proves who sent this.
    if verified.context.sender_id != request.node_id {
        return Err(ApiError::new(
            "forbidden",
            StatusCode::FORBIDDEN,
            "a node may only report its own capability state",
        ));
    }
    if request.xp_version.trim().is_empty() {
        return Err(ApiError::invalid_request("xp_version is empty"));
    }
    if request.capabilities.len() > MAX_REPORTED_CAPABILITIES
        || request
            .capabilities
            .iter()
            .any(|capability| capability.is_empty() || capability.len() > MAX_CAPABILITY_LEN)
    {
        return Err(ApiError::invalid_request(
            "capability list is out of bounds",
        ));
    }
    if !is_leader(&raft_metrics(&state)) {
        return Err(ApiError::invalid_request("not leader"));
    }
    if state
        .store
        .lock()
        .await
        .get_node(&request.node_id)
        .is_none()
    {
        return Err(ApiError::not_found(format!(
            "node not found: {}",
            request.node_id
        )));
    }

    let report = crate::capability_evidence::NodeCapabilityReport::new(
        request.xp_version.trim(),
        request.capabilities,
        Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
    );
    let note = crate::capability_evidence::encode_capability_report_note(&request.node_id, &report)
        .map_err(|error| ApiError::internal(format!("encode capability report: {error}")))?;
    let _ = raft_write(&state, DesiredStateCommand::CompatNoop { note }).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Report this node's own capability state to the current leader.
///
/// Best effort: a cluster without an elected leader, or one running an older release, simply does
/// not record evidence. No lifecycle decision depends on a report succeeding.
pub(crate) fn spawn_capability_report_worker(state: AppState) {
    tokio::spawn(async move {
        // Let the cluster settle before the first attempt so a fresh process does not race the
        // election that will accept its report.
        tokio::time::sleep(Duration::from_secs(20)).await;
        let mut reported: Option<(String, String, String)> = None;
        loop {
            if let Err(error) = report_own_capabilities_once(&state, &mut reported).await {
                tracing::debug!(%error, "capability self-report skipped");
            }
            tokio::time::sleep(Duration::from_secs(10 * 60)).await;
        }
    });
}

async fn report_own_capabilities_once(
    state: &AppState,
    reported: &mut Option<(String, String, String)>,
) -> anyhow::Result<()> {
    let metrics = raft_metrics(state);
    let leader_id = metrics
        .current_leader
        .ok_or_else(|| anyhow::anyhow!("cluster has no elected leader"))?;
    let local_raft_id = crate::raft::types::raft_node_id_from_ulid(&state.cluster.node_id)?;
    if leader_id == local_raft_id {
        // The leader already owns this evidence about itself; write it locally instead of sending.
        record_locally(state).await?;
        return Ok(());
    }
    let Some(leader_base_url) = metrics
        .membership_config
        .membership()
        .get_node(&leader_id)
        .map(|meta| meta.api_base_url.clone())
    else {
        anyhow::bail!("leader is not part of the current membership view");
    };
    let capabilities = own_capabilities(state).await;
    let identity = (
        leader_base_url.clone(),
        crate::version::VERSION.to_string(),
        capabilities.join(","),
    );
    if reported.as_ref() == Some(&identity) {
        return Ok(());
    }
    let Some(ca_key_pem) = state.cluster_ca_key_pem.as_deref() else {
        anyhow::bail!("cluster CA key is not available");
    };
    let body = serde_json::to_vec(&serde_json::json!({
        "node_id": state.cluster.node_id,
        "xp_version": crate::version::VERSION,
        "capabilities": capabilities,
    }))?;
    let peer = crate::control_plane_mesh::MeshPeerTarget {
        node_id: String::new(),
        node_name: String::new(),
        mesh_base_url: None,
        endpoint_transport: None,
        endpoint_fingerprint: None,
        mesh_reason: crate::mesh_telemetry::MeshPeerReason::MissingEndpoint,
        public_base_url: leader_base_url,
    };
    let request = crate::control_plane_mesh::MeshRequest {
        method: reqwest::Method::POST,
        path_and_query: "/api/admin/_internal/report-node-state".to_string(),
        content_type: Some("application/json".to_string()),
        body,
        total_budget: Duration::from_secs(10),
        allow_ambiguous_fallback: false,
        request_id: crate::id::new_ulid_string(),
        route: crate::internal_auth::InternalRoute::MeshV2,
        cluster_id: state.cluster.cluster_id.clone(),
        sender_id: state.cluster.node_id.clone(),
        updates_active_path: false,
    };
    let response = state
        .mesh_client
        .send_peer_direct_request(
            &peer,
            crate::control_plane_mesh::PeerDirectPath::ApiBaseUrl,
            request,
            ca_key_pem,
            &state.cluster_ca_pem,
        )
        .await?;
    if !response.status().is_success() {
        anyhow::bail!("leader rejected capability report ({})", response.status());
    }
    *reported = Some(identity);
    Ok(())
}

async fn record_locally(state: &AppState) -> anyhow::Result<()> {
    let report = crate::capability_evidence::NodeCapabilityReport::new(
        crate::version::VERSION,
        own_capabilities(state).await,
        Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
    );
    if let Some(existing) = state
        .store
        .lock()
        .await
        .get_node_capability_report(&state.cluster.node_id)
        && existing.xp_version == report.xp_version
        && existing.capabilities == report.capabilities
    {
        return Ok(());
    }
    let note =
        crate::capability_evidence::encode_capability_report_note(&state.cluster.node_id, &report)?;
    let _ = raft_write(state, DesiredStateCommand::CompatNoop { note })
        .await
        .map_err(|error| anyhow::anyhow!("{}: {}", error.code, error.message))?;
    Ok(())
}

/// The capability list this build advertises, minus per-request readiness facts. Those are
/// deliberately excluded: evidence must not claim a transient quality the node may not have later.
async fn own_capabilities(_state: &AppState) -> Vec<String> {
    let mut capabilities = capabilities::base_capabilities()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    capabilities.sort();
    capabilities.dedup();
    capabilities
}
