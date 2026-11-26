use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use axum::http::StatusCode;
use chrono::Utc;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use super::{
    ApiError, AppState, MeshCapabilityProbeResponse, raft_metrics,
    send_mesh_internal_capability_read,
};
use crate::{
    capability_evidence::{UnsupportedReason, VoterCapabilityState, classify_without_response},
    control_plane_mesh::{MeshPeerTarget, MeshRequest, PeerDirectPath},
    domain::Node,
};

pub(super) const MEMBERSHIP_LIFECYCLE_CAPABILITY: &str = "cluster.membership-lifecycle-v1";
pub(super) const STALE_LEARNER_RETIREMENT_CAPABILITY: &str = "cluster.stale-learner-retirement-v1";
pub(super) const REVERSE_ASSIGNMENT_CAPABILITY: &str = "cluster.mesh-reverse-assignment-v1";
pub(super) const MESH_GATE_CAPABILITY: &str = "cluster.mesh-gate-v1";
const CAPABILITY_PROBE_BUDGET: Duration = Duration::from_secs(5);
const MAX_CAPABILITY_RESPONSE_BYTES: usize = 64 * 1024;
const LEGACY_CAPABILITIES_PATH: &str = "/api/capabilities";

fn remaining_probe_budget(started: Instant) -> Option<Duration> {
    let remaining = CAPABILITY_PROBE_BUDGET.saturating_sub(started.elapsed());
    (!remaining.is_zero()).then_some(remaining)
}

async fn read_capability_response(
    response: reqwest::Response,
    budget: Duration,
) -> Option<Response> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_CAPABILITY_RESPONSE_BYTES as u64)
    {
        return None;
    }
    let bytes = tokio::time::timeout(budget, async move {
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.ok()?;
            let next_len = bytes.len().checked_add(chunk.len())?;
            if next_len > MAX_CAPABILITY_RESPONSE_BYTES {
                return None;
            }
            bytes.extend_from_slice(&chunk);
        }
        Some(bytes)
    })
    .await
    .ok()
    .flatten()?;
    serde_json::from_slice(&bytes).ok()
}

#[derive(Deserialize)]
struct Response {
    capabilities: Vec<String>,
}

#[derive(Debug, Clone)]
struct VoterCapabilityPeer {
    raft_node_id: u64,
    node: Node,
}

/// One retained voter's capability verdict, reported back to callers that must explain a decision.
#[derive(Debug, Clone)]
pub(crate) struct VoterClassification {
    pub(crate) raft_node_id: u64,
    pub(crate) node_id: String,
    pub(crate) state: VoterCapabilityState,
}

impl Serialize for VoterClassification {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut out = serializer.serialize_struct("VoterClassification", 4)?;
        out.serialize_field("raft_node_id", &self.raft_node_id)?;
        out.serialize_field("node_id", &self.node_id)?;
        out.serialize_field("state", self.state.as_str())?;
        out.serialize_field(
            "reason",
            &match &self.state {
                VoterCapabilityState::Unsupported(reason) => Some(reason.as_str()),
                _ => None,
            },
        )?;
        out.end()
    }
}

pub(super) async fn require_membership_lifecycle_on_voters(
    state: &AppState,
) -> Result<(), ApiError> {
    require_capability_on_voters(state, MEMBERSHIP_LIFECYCLE_CAPABILITY, &BTreeSet::new()).await
}

pub(super) async fn require_membership_lifecycle_on_retained_voters(
    state: &AppState,
    excluded_voter_id: u64,
) -> Result<(), ApiError> {
    require_capability_on_voters(
        state,
        MEMBERSHIP_LIFECYCLE_CAPABILITY,
        &BTreeSet::from([excluded_voter_id]),
    )
    .await
}

/// Require the lifecycle capability from every retained voter except the previewed target and the
/// explicitly confirmed unreachable ones. Each confirmed id must classify as `Unreachable`; an
/// `Unsupported` verdict rejects the request so a live but downgraded voter cannot be evicted.
pub(super) async fn require_membership_lifecycle_excluding(
    state: &AppState,
    excluded_voter_ids: &BTreeSet<u64>,
) -> Result<Vec<VoterClassification>, ApiError> {
    let classifications =
        classify_capability_on_voters(state, MEMBERSHIP_LIFECYCLE_CAPABILITY).await?;
    for entry in &classifications {
        check_classification(entry, excluded_voter_ids)?;
    }
    Ok(classifications)
}

pub(super) async fn require_stale_learner_retirement_on_voters(
    state: &AppState,
) -> Result<(), ApiError> {
    require_capability_on_voters(state, STALE_LEARNER_RETIREMENT_CAPABILITY, &BTreeSet::new()).await
}

pub(super) async fn require_reverse_assignment_on_voters(state: &AppState) -> Result<(), ApiError> {
    require_capability_on_voters(state, REVERSE_ASSIGNMENT_CAPABILITY, &BTreeSet::new()).await
}

pub(super) async fn require_mesh_gate_on_voters(state: &AppState) -> Result<(), ApiError> {
    require_capability_on_voters_with_probe(state, MESH_GATE_CAPABILITY, &BTreeSet::new(), true)
        .await
}

pub(super) async fn require_xmux_settings_on_members(state: &AppState) -> Result<(), ApiError> {
    require_capability_on_voters_with_probe(
        state,
        "admin.endpoint-vless-xmux-settings-v1",
        &BTreeSet::new(),
        true,
    )
    .await
}

pub(super) async fn require_xmux_settings_on_node(
    state: &AppState,
    node: &Node,
) -> Result<(), ApiError> {
    if public_capability_supports(
        state,
        node,
        "admin.endpoint-vless-xmux-settings-v1",
        Instant::now(),
    )
    .await
    {
        Ok(())
    } else {
        Err(ApiError::new(
            "coordinated_upgrade_required",
            StatusCode::CONFLICT,
            "restore target must support XMUX settings while custom policies exist",
        ))
    }
}

pub(super) async fn lock_xmux_settings_change(
    state: &AppState,
) -> Result<Option<tokio::sync::OwnedMutexGuard<()>>, ApiError> {
    if !super::is_leader(&raft_metrics(state)) {
        return Err(ApiError::conflict(
            "XMUX settings must be changed on the current cluster leader",
        ));
    }
    let guard = crate::raft_membership_guard::membership_operation_gate()
        .lock_owned()
        .await;
    if !super::is_leader(&raft_metrics(state)) {
        return Err(ApiError::conflict(
            "cluster leadership changed; retry XMUX settings on the current leader",
        ));
    }
    state
        .raft
        .ensure_linearizable()
        .await
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    if state
        .store
        .lock()
        .await
        .state()
        .active_membership_operation()
        .is_some()
    {
        return Err(ApiError::new(
            "coordinated_upgrade_required",
            StatusCode::CONFLICT,
            "XMUX settings cannot change while a membership operation is active",
        ));
    }
    require_xmux_settings_on_members(state).await?;
    Ok(Some(guard))
}

async fn require_capability_on_voters(
    state: &AppState,
    capability: &str,
    excluded_voter_ids: &BTreeSet<u64>,
) -> Result<(), ApiError> {
    require_capability_on_voters_with_probe(state, capability, excluded_voter_ids, false).await
}

async fn require_capability_on_voters_with_probe(
    state: &AppState,
    capability: &str,
    excluded_voter_ids: &BTreeSet<u64>,
    public_only: bool,
) -> Result<(), ApiError> {
    let classifications =
        classify_capability_on_voters_with_probe(state, capability, public_only).await?;
    for entry in &classifications {
        check_classification(entry, excluded_voter_ids)?;
    }
    Ok(())
}

/// An exclusion can only ever excuse a voter that did not answer. A voter that answered without the
/// capability is a coordinated-upgrade blocker no matter what the operator named, because removing
/// it would let a downgraded node drop out of the barrier.
pub(super) fn check_classification(
    entry: &VoterClassification,
    excluded_voter_ids: &BTreeSet<u64>,
) -> Result<(), ApiError> {
    match &entry.state {
        VoterCapabilityState::Supported => Ok(()),
        VoterCapabilityState::Unsupported(reason) => {
            Err(unsupported_capability_error(entry, *reason))
        }
        VoterCapabilityState::Unreachable => {
            if excluded_voter_ids.contains(&entry.raft_node_id) {
                Ok(())
            } else {
                Err(unreachable_voter_error(entry))
            }
        }
    }
}

fn unsupported_capability_error(
    entry: &VoterClassification,
    reason: UnsupportedReason,
) -> ApiError {
    ApiError::new(
        "coordinated_upgrade_required",
        StatusCode::CONFLICT,
        format!(
            "retained voter {} ({}) answered and does not expose the required capability ({})",
            entry.raft_node_id,
            entry.node_id,
            reason.as_str()
        ),
    )
}

fn unreachable_voter_error(entry: &VoterClassification) -> ApiError {
    ApiError::new(
        "staged_join_capability_unavailable",
        StatusCode::SERVICE_UNAVAILABLE,
        format!(
            "cannot verify capability support on retained voter {} ({}): no verified signed \
             response and no fresh persisted evidence; name it in --confirmed-unreachable only \
             after confirming that host is permanently gone",
            entry.raft_node_id, entry.node_id
        ),
    )
}

async fn classify_capability_on_voters(
    state: &AppState,
    capability: &str,
) -> Result<Vec<VoterClassification>, ApiError> {
    classify_capability_on_voters_with_probe(state, capability, false).await
}

async fn classify_capability_on_voters_with_probe(
    state: &AppState,
    capability: &str,
    public_only: bool,
) -> Result<Vec<VoterClassification>, ApiError> {
    let metrics = raft_metrics(state);
    let membership = metrics.membership_config.membership();
    let mut voter_ids = membership.voter_ids().collect::<BTreeSet<_>>();
    if public_only {
        voter_ids.extend(membership.nodes().map(|(node_id, _)| *node_id));
    }
    let local_node_id = crate::raft::types::raft_node_id_from_ulid(&state.cluster.node_id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    voter_ids.remove(&local_node_id);

    let (nodes_by_raft_node_id, reports_by_node_id) = {
        let store = state.store.lock().await;
        let nodes = store
            .list_nodes()
            .into_iter()
            .filter_map(|node| {
                crate::raft::types::raft_node_id_from_ulid(&node.node_id)
                    .ok()
                    .map(|raft_node_id| (raft_node_id, node))
            })
            .collect::<BTreeMap<_, _>>();
        let reports = nodes
            .values()
            .filter_map(|node| {
                store
                    .get_node_capability_report(&node.node_id)
                    .map(|report| (node.node_id.clone(), report))
            })
            .collect::<BTreeMap<_, _>>();
        (nodes, reports)
    };
    let peers = voter_ids
        .iter()
        .filter_map(|raft_node_id| {
            membership.get_node(raft_node_id).and_then(|_| {
                nodes_by_raft_node_id
                    .get(raft_node_id)
                    .cloned()
                    .map(|node| VoterCapabilityPeer {
                        raft_node_id: *raft_node_id,
                        node,
                    })
            })
        })
        .collect::<Vec<_>>();
    if peers.len() != voter_ids.len() {
        return Err(ApiError::new(
            "coordinated_upgrade_required",
            StatusCode::CONFLICT,
            "every retained voter must expose valid Raft member metadata and DesiredState \
             mapping before membership changes",
        ));
    }

    let mut classifications = Vec::with_capacity(peers.len());
    for peer in peers {
        let probed = if public_only {
            classify_from_public_probe(state, &peer.node, capability, Instant::now()).await
        } else {
            classify_from_signed_probe(state, &peer.node, capability).await
        };
        // Persisted evidence is only ever consulted when the voter produced no verified signed
        // response at all. A verified answer without the capability stays terminal.
        let state = match probed {
            VoterCapabilityState::Unreachable => classify_without_response(
                reports_by_node_id.get(&peer.node.node_id),
                capability,
                Utc::now(),
            ),
            answered => answered,
        };
        classifications.push(VoterClassification {
            raft_node_id: peer.raft_node_id,
            node_id: peer.node.node_id,
            state,
        });
    }
    Ok(classifications)
}

async fn classify_from_signed_probe(
    state: &AppState,
    node: &Node,
    capability: &str,
) -> VoterCapabilityState {
    let started = Instant::now();
    let response = match send_mesh_internal_capability_read(
        state,
        &state.mesh_client,
        node,
        CAPABILITY_PROBE_BUDGET,
    )
    .await
    {
        Ok(response) => response,
        // Transport failure, timeout, TLS error, or an edge error: this voter did not answer.
        Err(_) => return VoterCapabilityState::Unreachable,
    };
    // Only a predecessor's unsigned 404 proves that it predates the signed route. A signed 404,
    // a protocol error, or a Mesh transport failure is terminal and must not move this probe onto
    // a public path.
    let (response, mesh_remaining) = match response {
        MeshCapabilityProbeResponse::Verified { response, deadline } => {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return VoterCapabilityState::Unreachable;
            }
            (response, Some(remaining))
        }
        MeshCapabilityProbeResponse::PredecessorNotFound => {
            let Some(remaining) = remaining_probe_budget(started) else {
                return VoterCapabilityState::Unreachable;
            };
            let api_base_url = node.api_base_url.trim().trim_end_matches('/');
            if api_base_url.is_empty() {
                return VoterCapabilityState::Unreachable;
            }
            let url = format!("{api_base_url}{LEGACY_CAPABILITIES_PATH}");
            let Ok(response) = state
                .mesh_client
                .direct()
                .get(url)
                .timeout(remaining)
                .send()
                .await
            else {
                return VoterCapabilityState::Unreachable;
            };
            if !response.status().is_success() {
                return VoterCapabilityState::Unsupported(UnsupportedReason::PreRoute404);
            }
            let Some(remaining) = remaining_probe_budget(started) else {
                return VoterCapabilityState::Unreachable;
            };
            return match read_capability_response(response, remaining).await {
                Some(body) if body.capabilities.iter().any(|item| item == capability) => {
                    VoterCapabilityState::Supported
                }
                Some(_) => VoterCapabilityState::Unsupported(UnsupportedReason::PreRoute404),
                // A predecessor answered over HTTP but the body was unusable; treat it as an
                // answer that does not prove support.
                None => VoterCapabilityState::Unsupported(UnsupportedReason::PreRoute404),
            };
        }
    };
    if !response.status().is_success() {
        return VoterCapabilityState::Unsupported(UnsupportedReason::VerifiedMissing);
    }
    match mesh_remaining.or_else(|| remaining_probe_budget(started)) {
        Some(remaining) => match read_capability_response(response, remaining).await {
            Some(body) if body.capabilities.iter().any(|item| item == capability) => {
                VoterCapabilityState::Supported
            }
            Some(_) => VoterCapabilityState::Unsupported(UnsupportedReason::VerifiedMissing),
            None => VoterCapabilityState::Unreachable,
        },
        None => VoterCapabilityState::Unreachable,
    }
}

async fn classify_from_public_probe(
    state: &AppState,
    node: &Node,
    capability: &str,
    started: Instant,
) -> VoterCapabilityState {
    let Some(remaining) = remaining_probe_budget(started) else {
        return VoterCapabilityState::Unreachable;
    };
    let api_base_url = node.api_base_url.trim().trim_end_matches('/');
    if api_base_url.is_empty() {
        return VoterCapabilityState::Unreachable;
    }
    let peer = MeshPeerTarget {
        node_id: node.node_id.clone(),
        node_name: node.node_name.clone(),
        mesh_base_url: None,
        endpoint_transport: None,
        endpoint_fingerprint: None,
        mesh_reason: crate::mesh_telemetry::MeshPeerReason::MissingEndpoint,
        public_base_url: api_base_url.to_string(),
    };
    let request = MeshRequest {
        method: reqwest::Method::GET,
        path_and_query: "/api/admin/_internal/capabilities".to_string(),
        content_type: None,
        body: Vec::new(),
        total_budget: remaining,
        allow_ambiguous_fallback: false,
        request_id: crate::id::new_ulid_string(),
        route: crate::internal_auth::InternalRoute::MeshV2,
        cluster_id: state.cluster.cluster_id.clone(),
        sender_id: state.cluster.node_id.clone(),
        updates_active_path: false,
    };
    let Some(ca_key_pem) = state.cluster_ca_key_pem.as_deref() else {
        return VoterCapabilityState::Unreachable;
    };
    let Ok(response) = state
        .mesh_client
        .send_peer_direct_request(
            &peer,
            PeerDirectPath::ApiBaseUrl,
            request,
            ca_key_pem,
            &state.cluster_ca_pem,
        )
        .await
    else {
        return VoterCapabilityState::Unreachable;
    };
    if !response.status().is_success() {
        return VoterCapabilityState::Unsupported(UnsupportedReason::VerifiedMissing);
    }
    let Some(remaining) = remaining_probe_budget(started) else {
        return VoterCapabilityState::Unreachable;
    };
    match read_capability_response(response, remaining).await {
        Some(body) if body.capabilities.iter().any(|item| item == capability) => {
            VoterCapabilityState::Supported
        }
        Some(_) => VoterCapabilityState::Unsupported(UnsupportedReason::VerifiedMissing),
        None => VoterCapabilityState::Unreachable,
    }
}

async fn public_capability_supports(
    state: &AppState,
    node: &Node,
    capability: &str,
    started: Instant,
) -> bool {
    classify_from_public_probe(state, node, capability, started).await
        == VoterCapabilityState::Supported
}

#[cfg(test)]
#[path = "join_capability_classification_tests.rs"]
mod classification_tests;
