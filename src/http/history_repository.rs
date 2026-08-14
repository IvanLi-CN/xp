use axum::{
    Json,
    extract::{Extension, Query},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use x25519_dalek::{PublicKey as X25519DalekPublicKey, StaticSecret};

use crate::{
    history_sync::{MAX_RESPONSE_WIRE_BYTES, PayloadEncoding},
    http::{ApiError, ApiJson, AppState, InternalSignatureAuth},
    state::history_repository::{
        control::{
            RepositoryCapacity, RepositoryLifecycle, RepositoryMember, RepositoryMembership,
        },
        identity::{Ed25519PublicKey, RepositoryNodeId, RepositoryNodeIdentity, X25519PublicKey},
        query::{HistoryQuery, QueryCandidate, QuerySelector},
        replica::{
            LocalQueryMetadata, RepositoryHistoryQueryResponse, RepositoryRepairBatch,
            RepositoryReplicaGap, RepositoryReplicaSummary, RepositoryRuntimeError,
            RepositoryRuntimeStatus, RepositorySyncReceipt, RepositoryTombstoneAcknowledgement,
        },
    },
};

const MAX_HISTORY_SYNC_BASE64_BYTES: usize = MAX_RESPONSE_WIRE_BYTES.div_ceil(3) * 4;
const MAX_REPAIR_REQUEST_IDS: usize = 64;
const REPOSITORY_ED25519_KEY_CONTEXT: &[u8] = b"xp-history-repository-ed25519-v1\0";
const REPOSITORY_X25519_KEY_CONTEXT: &[u8] = b"xp-history-repository-x25519-v1\0";
pub(super) const INTERNAL_HISTORY_REPOSITORY_RELAY: &str =
    "/api/admin/_internal/history-repository/relay";
pub(super) const INTERNAL_HISTORY_REPOSITORY_RELAY_DELIVER: &str =
    "/api/admin/_internal/history-repository/relay-deliver";

mod worker;
pub(crate) use worker::spawn_repository_replica_worker;

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct RepositoryHistoryQuery {
    start_unix_seconds: u64,
    end_unix_seconds: u64,
    page_size: usize,
    #[serde(default)]
    page_cursor: Option<String>,
    #[serde(default)]
    subject_node_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RepositoryInitialBackfillQuery {
    #[serde(default)]
    page_cursor: Option<String>,
    #[serde(default)]
    page_size: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RepositorySummaryQuery {
    #[serde(default)]
    after_segment_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReplaceRepositoryMembershipRequest {
    node_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct AdminHistoryRepositoriesResponse {
    configured: bool,
    partial: bool,
    unreachable_node_ids: Vec<String>,
    items: Vec<AdminHistoryRepositoryItem>,
}

#[derive(Debug, Serialize)]
struct AdminHistoryRepositoryItem {
    member: RepositoryMember,
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime: Option<RepositoryRuntimeStatus>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct RepositorySyncRequest {
    identity: RepositoryNodeIdentity,
    wire_base64: String,
    canonical_len: usize,
    #[serde(default)]
    wire_encoding: RepositorySyncWireEncoding,
    #[serde(default)]
    gaps: Vec<RepositoryReplicaGap>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum RepositorySyncWireEncoding {
    #[default]
    Identity,
    Zstd,
}

impl RepositorySyncRequest {
    pub(super) fn with_wire(
        identity: RepositoryNodeIdentity,
        wire: &[u8],
        gaps: Vec<RepositoryReplicaGap>,
    ) -> Result<Self, anyhow::Error> {
        let canonical_len = wire.len();
        let (payload_encoding, encoded) = crate::history_sync::encode_payload(wire.to_vec())?;
        Ok(Self {
            identity,
            wire_base64: URL_SAFE_NO_PAD.encode(encoded),
            canonical_len,
            wire_encoding: payload_encoding.into(),
            gaps,
        })
    }

    fn decode_wire(&self) -> Result<Vec<u8>, ApiError> {
        if self.wire_base64.len() > MAX_HISTORY_SYNC_BASE64_BYTES {
            return Err(ApiError::invalid_request(
                "repository segment exceeds wire limit",
            ));
        }
        let encoded = URL_SAFE_NO_PAD
            .decode(&self.wire_base64)
            .map_err(|_| ApiError::invalid_request("repository segment is not base64url"))?;
        crate::history_sync::decode_payload(self.wire_encoding.into(), &encoded, self.canonical_len)
            .map_err(|_| ApiError::invalid_request("repository segment encoding is invalid"))
    }
}

impl From<PayloadEncoding> for RepositorySyncWireEncoding {
    fn from(value: PayloadEncoding) -> Self {
        match value {
            PayloadEncoding::Identity => Self::Identity,
            PayloadEncoding::ZstandardLevel1 => Self::Zstd,
        }
    }
}

impl From<RepositorySyncWireEncoding> for PayloadEncoding {
    fn from(value: RepositorySyncWireEncoding) -> Self {
        match value {
            RepositorySyncWireEncoding::Identity => Self::Identity,
            RepositorySyncWireEncoding::Zstd => Self::ZstandardLevel1,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct RepositoryRepairRequest {
    segment_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct RepositoryTombstoneAcknowledgementRequest {
    acknowledgements: Vec<RepositoryTombstoneAcknowledgement>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct RepositoryRelayRequest {
    target_repository_id: String,
    source_repository_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    relay_repository_id: Option<String>,
    frame: crate::history_sync::RelayFrame,
}

pub(super) async fn admin_internal_receive_history_repository_segment(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<RepositorySyncRequest>,
) -> Result<Json<RepositorySyncReceipt>, ApiError> {
    let Some(Extension(internal)) = internal else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    let Some(verified) = internal.verified else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    if !repository_identity_matches_sender(&request.identity, &verified.context.sender_id)
        || !identity_is_pinned_for_node(&state, &request.identity).await?
    {
        return Err(ApiError::unauthorized(
            "repository segment identity is not pinned for the authenticated sender",
        ));
    }
    let ready_repository_ids = ready_repository_ids(&state).await?;
    let accepts_source = state
        .repository_replica
        .lock()
        .await
        .accepts_source(
            request.identity.node_id().as_str(),
            &ready_repository_ids,
            &state.cluster.node_id,
        )
        .map_err(repository_error)?;
    if !accepts_source {
        return Err(ApiError::conflict(
            "repository is not a rendezvous collector for this source",
        ));
    }
    let wire = request.decode_wire()?;
    if !source_gaps_match_identity(&request.gaps, request.identity.node_id().as_str()) {
        return Err(ApiError::invalid_request(
            "history source gaps do not match the authenticated source",
        ));
    }
    let receipt = {
        let mut runtime = state.repository_replica.lock().await;
        if !request.gaps.is_empty() {
            runtime
                .merge_replica_gaps(&request.gaps)
                .map_err(repository_error)?;
        }
        runtime
            .receive_wire_from_repository(
                &state.cluster.cluster_id,
                &request.identity,
                &wire,
                u64::try_from(Utc::now().timestamp()).unwrap_or_default(),
                &ready_repository_ids,
                &state.cluster.node_id,
            )
            .map_err(repository_error)?
    };
    state
        .repository_replica
        .lock()
        .await
        .record_collection_cycle(
            request.identity.node_id().as_str(),
            &ready_repository_ids,
            &state.cluster.node_id,
            true,
        )
        .map_err(repository_error)?;
    worker::propagate_tombstone_acknowledgements(
        &state,
        &ready_repository_ids,
        receipt.tombstone_acknowledgements().to_vec(),
    )
    .await
    .map_err(|error| ApiError::gateway_timeout(error.to_string()))?;
    Ok(Json(receipt))
}

pub(super) async fn admin_internal_initial_history_repository_backfill(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    Query(query): Query<RepositoryInitialBackfillQuery>,
) -> Result<Json<worker::RepositoryInitialBackfillPage>, ApiError> {
    ensure_cluster_sender(&state, internal).await?;
    worker::initial_backfill_page(&state, query.page_cursor.as_deref(), query.page_size)
        .await
        .map(Json)
        .map_err(|error| ApiError::invalid_request(error.to_string()))
}

fn repository_identity_matches_sender(identity: &RepositoryNodeIdentity, sender_id: &str) -> bool {
    identity.node_id().as_str() == sender_id
}

fn source_gaps_match_identity(gaps: &[RepositoryReplicaGap], source_node_id: &str) -> bool {
    gaps.len() <= MAX_REPAIR_REQUEST_IDS
        && gaps
            .iter()
            .all(|gap| gap.source_node_id == source_node_id && gap.permanent)
}

fn ordinary_source_relay_batch_is_valid(
    batch: &RepositoryRepairBatch,
    source_node_id: &str,
) -> bool {
    batch
        .segments
        .iter()
        .all(|segment| segment.identity.node_id().as_str() == source_node_id)
        && source_gaps_match_identity(&batch.gaps, source_node_id)
}

pub(super) async fn identity_is_pinned_for_node(
    state: &AppState,
    identity: &RepositoryNodeIdentity,
) -> Result<bool, ApiError> {
    let node_id = identity.node_id().clone();
    let configured_identity = {
        let store = state.store.lock().await;
        if !store.state().nodes.contains_key(node_id.as_str()) {
            return Ok(false);
        }
        store
            .state()
            .repository_membership
            .as_ref()
            .and_then(|membership| membership.repository(&node_id))
            .map(|member| member.identity().clone())
    };
    let expected = match configured_identity {
        Some(identity) => identity,
        None => derived_repository_identity(state, node_id)?,
    };
    Ok(&expected == identity)
}

fn relay_frame_matches_source(
    frame: &crate::history_sync::RelayFrame,
    source_keypair: crate::history_sync::RelayKeypair,
) -> bool {
    frame.sender_public_key() == source_keypair.public_key()
}

pub(super) async fn admin_internal_history_repository_summary(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    Query(query): Query<RepositorySummaryQuery>,
) -> Result<Json<RepositoryReplicaSummary>, ApiError> {
    ensure_syncing_or_ready_repository_sender(&state, internal).await?;
    if query
        .after_segment_id
        .as_deref()
        .is_some_and(|id| id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err(ApiError::invalid_request(
            "invalid repository summary cursor",
        ));
    }
    let summary = state
        .repository_replica
        .lock()
        .await
        .replication_summary_after(query.after_segment_id.as_deref())
        .map_err(repository_error)?;
    Ok(Json(summary))
}

pub(super) async fn admin_internal_history_repository_status(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
) -> Result<Json<RepositoryRuntimeStatus>, ApiError> {
    let Some(Extension(internal)) = internal else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    if internal.verified.is_none() {
        return Err(ApiError::unauthorized("internal auth required"));
    }
    Ok(Json(local_repository_runtime_status(&state).await?))
}

pub(super) async fn admin_internal_history_repository_repair(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<RepositoryRepairRequest>,
) -> Result<Json<RepositoryRepairBatch>, ApiError> {
    ensure_syncing_or_ready_repository_sender(&state, internal).await?;
    if request.segment_ids.len() > MAX_REPAIR_REQUEST_IDS {
        return Err(ApiError::invalid_request(
            "too many repository repair segment ids",
        ));
    }
    let response = state
        .repository_replica
        .lock()
        .await
        .repair_batch(&request.segment_ids)
        .map_err(repository_error)?;
    Ok(Json(response))
}

pub(super) async fn admin_internal_query_history_repository(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<RepositoryHistoryQuery>,
) -> Result<Json<RepositoryHistoryQueryResponse>, ApiError> {
    ensure_ready_repository_sender(&state, internal).await?;
    let ready_repository_ids = ready_repository_ids(&state).await?;
    if !ready_repository_ids
        .iter()
        .any(|repository_id| repository_id == &state.cluster.node_id)
    {
        return Err(ApiError::conflict("repository receiver is not ready"));
    }
    let query = HistoryQuery::new(
        request.start_unix_seconds,
        request.end_unix_seconds,
        request.page_size,
    )
    .and_then(|query| query.with_page_cursor(request.page_cursor.as_deref()))
    .and_then(|query| query.with_subject_node_id(request.subject_node_id.as_deref()))
    .map_err(|error| ApiError::invalid_request(error.to_string()))?;
    let now = u64::try_from(Utc::now().timestamp()).unwrap_or_default();
    let mut runtime = state.repository_replica.lock().await;
    runtime
        .prepare_for_replication(now)
        .map_err(repository_error)?;
    let response = runtime
        .query(
            &state.cluster.node_id,
            query,
            LocalQueryMetadata::current_window(now),
        )
        .map_err(repository_error)?;
    Ok(Json(response))
}

pub(super) async fn admin_internal_forward_history_repository_relay(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<RepositoryRelayRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let sender = ensure_cluster_sender(&state, internal).await?;
    if request.source_repository_id != sender {
        return Err(ApiError::unauthorized(
            "relay source does not match the authenticated sender",
        ));
    }
    if request.relay_repository_id.is_some() {
        return Err(ApiError::invalid_request(
            "nested repository relay is not supported",
        ));
    }
    let (ready_repository_ids, peers) = worker::ready_repository_peers(&state)
        .await
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    let target = peers
        .iter()
        .find(|peer| peer.node_id == request.target_repository_id)
        .ok_or_else(|| ApiError::invalid_request("relay target is not a ready repository"))?;
    if !ready_repository_ids
        .iter()
        .any(|repository_id| repository_id == &request.target_repository_id)
    {
        return Err(ApiError::invalid_request("relay target is not ready"));
    }
    let body = serde_json::to_vec(&RepositoryRelayRequest {
        target_repository_id: request.target_repository_id,
        source_repository_id: request.source_repository_id,
        relay_repository_id: Some(state.cluster.node_id.clone()),
        frame: request.frame,
    })
    .map_err(|error| ApiError::internal(error.to_string()))?;
    worker::repository_mesh_request::<serde_json::Value>(
        &state,
        target,
        axum::http::Method::POST,
        INTERNAL_HISTORY_REPOSITORY_RELAY_DELIVER,
        body,
    )
    .await
    .map_err(|error| ApiError::gateway_timeout(error.to_string()))?;
    Ok(Json(serde_json::json!({})))
}

pub(super) async fn admin_internal_deliver_history_repository_relay(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<RepositoryRelayRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let sender = ensure_cluster_sender(&state, internal).await?;
    if request.relay_repository_id.as_deref() != Some(sender.as_str()) {
        return Err(ApiError::unauthorized(
            "relay forwarding identity does not match the authenticated sender",
        ));
    }
    if request.target_repository_id != state.cluster.node_id {
        return Err(ApiError::invalid_request(
            "relay delivery target does not match this repository",
        ));
    }
    let ready_repository_ids = ready_repository_ids(&state).await?;
    if !ready_repository_ids
        .iter()
        .any(|repository_id| repository_id == &state.cluster.node_id)
    {
        return Err(ApiError::conflict("relay target repository is not ready"));
    }
    {
        let store = state.store.lock().await;
        if !store
            .state()
            .nodes
            .contains_key(&request.source_repository_id)
        {
            return Err(ApiError::unauthorized(
                "relay source is not a cluster member",
            ));
        }
    }
    let source_keypair = worker::cluster_relay_keypair(&state, &request.source_repository_id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    if !relay_frame_matches_source(&request.frame, source_keypair) {
        return Err(ApiError::unauthorized(
            "relay frame sender does not match the claimed repository source",
        ));
    }
    let source_is_ready_repository = ready_repository_ids
        .iter()
        .any(|repository_id| repository_id == &request.source_repository_id);
    if !source_is_ready_repository {
        let accepts_source = state
            .repository_replica
            .lock()
            .await
            .accepts_source(
                &request.source_repository_id,
                &ready_repository_ids,
                &state.cluster.node_id,
            )
            .map_err(repository_error)?;
        if !accepts_source {
            return Err(ApiError::conflict(
                "relay target is not a rendezvous collector for this source",
            ));
        }
    }
    let keypair = worker::cluster_relay_keypair(&state, &state.cluster.node_id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let plaintext = request
        .frame
        .open(keypair, request.target_repository_id.as_bytes())
        .map_err(|error| ApiError::unauthorized(error.to_string()))?;
    let batch = RepositoryRepairBatch::from_relay_payload(&plaintext).map_err(repository_error)?;
    if !source_is_ready_repository
        && !ordinary_source_relay_batch_is_valid(&batch, &request.source_repository_id)
    {
        return Err(ApiError::unauthorized(
            "ordinary source relay may carry only its own history segments",
        ));
    }
    let mut acknowledgements = Vec::new();
    for segment in batch.segments {
        // The frame authenticates the forwarding repository; the segment authenticates its source.
        if !identity_is_pinned_for_node(&state, &segment.identity).await? {
            return Err(ApiError::unauthorized(
                "relayed repository segment identity is not pinned",
            ));
        }
        let receipt = state
            .repository_replica
            .lock()
            .await
            .receive_wire_from_repository(
                &state.cluster.cluster_id,
                &segment.identity,
                &segment.wire,
                u64::try_from(Utc::now().timestamp()).unwrap_or_default(),
                &ready_repository_ids,
                &state.cluster.node_id,
            )
            .map_err(repository_error)?;
        acknowledgements.extend(receipt.tombstone_acknowledgements().iter().cloned());
    }
    state
        .repository_replica
        .lock()
        .await
        .merge_replica_gaps(&batch.gaps)
        .map_err(repository_error)?;
    worker::propagate_tombstone_acknowledgements(&state, &ready_repository_ids, acknowledgements)
        .await
        .map_err(|error| ApiError::gateway_timeout(error.to_string()))?;
    Ok(Json(serde_json::json!({})))
}

pub(super) async fn admin_internal_acknowledge_history_repository_tombstones(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<RepositoryTombstoneAcknowledgementRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let sender = ensure_ready_repository_sender(&state, internal).await?;
    if request
        .acknowledgements
        .iter()
        .any(|acknowledgement| acknowledgement.repository_id() != sender)
    {
        return Err(ApiError::unauthorized(
            "repository tombstone acknowledgement sender does not match",
        ));
    }
    state
        .repository_replica
        .lock()
        .await
        .acknowledge_tombstones(&request.acknowledgements)
        .map_err(repository_error)?;
    Ok(Json(serde_json::json!({})))
}

pub(super) async fn admin_query_history_repository(
    Extension(state): Extension<AppState>,
    Query(request): Query<RepositoryHistoryQuery>,
) -> Result<Json<RepositoryHistoryQueryResponse>, ApiError> {
    let query = HistoryQuery::new(
        request.start_unix_seconds,
        request.end_unix_seconds,
        request.page_size,
    )
    .and_then(|query| query.with_page_cursor(request.page_cursor.as_deref()))
    .and_then(|query| query.with_subject_node_id(request.subject_node_id.as_deref()))
    .map_err(|error| ApiError::invalid_request(error.to_string()))?;
    let now = u64::try_from(Utc::now().timestamp()).unwrap_or_default();
    let (ready_repository_ids, peers) = worker::ready_repository_peers(&state)
        .await
        .unwrap_or_default();
    let local_is_ready = ready_repository_ids
        .iter()
        .any(|repository_id| repository_id == &state.cluster.node_id);
    let local_response = {
        let mut runtime = state.repository_replica.lock().await;
        runtime
            .prepare_for_replication(now)
            .map_err(repository_error)?;
        if local_is_ready {
            runtime
                .query(
                    &state.cluster.node_id,
                    query.clone(),
                    LocalQueryMetadata::current_window(now),
                )
                .map_err(repository_error)?
        } else {
            runtime
                .query_local_only(query.clone(), LocalQueryMetadata::current_window(now))
                .map_err(repository_error)?
        }
    };
    let body = serde_json::to_vec(&RepositoryHistoryQuery {
        start_unix_seconds: request.start_unix_seconds,
        end_unix_seconds: request.end_unix_seconds,
        page_size: request.page_size,
        page_cursor: request.page_cursor.clone(),
        subject_node_id: request.subject_node_id.clone(),
    })
    .map_err(|error| ApiError::internal(error.to_string()))?;
    let mut responses = vec![local_response];
    for peer in peers
        .iter()
        .filter(|peer| peer.node_id != state.cluster.node_id)
        .take(MAX_REPAIR_REQUEST_IDS)
    {
        if let Ok(response) = worker::repository_direct_request::<RepositoryHistoryQueryResponse>(
            &state,
            peer,
            axum::http::Method::POST,
            "/api/admin/_internal/history-repository/query",
            body.clone(),
        )
        .await
        {
            responses.push(response);
        }
    }
    let mut candidates = Vec::with_capacity(responses.len());
    for response in &responses {
        let plan = response.plan();
        let Some(coverage) = plan.coverage().cloned() else {
            continue;
        };
        let candidate = match plan.repository_id() {
            Some(repository_id) => QueryCandidate::ready(
                repository_id,
                coverage,
                plan.watermarks().iter().cloned(),
                plan.gaps().iter().cloned(),
                plan.clock_skew_seconds(),
            ),
            None => QueryCandidate::local(
                coverage,
                plan.watermarks().iter().cloned(),
                plan.gaps().iter().cloned(),
                plan.clock_skew_seconds(),
            ),
        }
        .map_err(|error| ApiError::internal(error.to_string()))?;
        candidates.push(candidate);
    }
    let selected = QuerySelector::select(&query, candidates)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let response = match selected.repository_id() {
        Some(repository_id) => responses
            .into_iter()
            .find(|response| response.plan().repository_id() == Some(repository_id))
            .ok_or_else(|| ApiError::internal("selected repository response is unavailable"))?,
        None => responses
            .into_iter()
            .find(|response| response.plan().repository_id().is_none())
            .ok_or_else(|| ApiError::internal("local history response is unavailable"))?,
    };
    Ok(Json(response))
}

pub(super) async fn admin_list_history_repositories(
    Extension(state): Extension<AppState>,
) -> Result<Json<AdminHistoryRepositoriesResponse>, ApiError> {
    let (membership, nodes) = {
        let store = state.store.lock().await;
        (
            store.state().repository_membership.clone(),
            store.list_nodes(),
        )
    };
    let Some(membership) = membership else {
        return Ok(Json(AdminHistoryRepositoriesResponse {
            configured: false,
            partial: false,
            unreachable_node_ids: Vec::new(),
            items: Vec::new(),
        }));
    };

    let nodes = nodes
        .into_iter()
        .map(|node| (node.node_id.clone(), node))
        .collect::<BTreeMap<_, _>>();
    let local_node_id = state.cluster.node_id.clone();
    let client = state.mesh_client.clone();
    let mut partial = false;
    let mut unreachable_node_ids = Vec::new();
    let mut items = Vec::with_capacity(membership.members().len());

    for member in membership.members() {
        let node_id = member.node_id().as_str().to_owned();
        let runtime = if node_id == local_node_id {
            Some(local_repository_runtime_status(&state).await?)
        } else if let Some(node) = nodes.get(&node_id) {
            match super::mesh::send_mesh_internal_read(
                &state,
                &client,
                node,
                "/api/admin/_internal/history-repository/status".to_owned(),
                std::time::Duration::from_secs(5),
            )
            .await
            {
                Ok(response) if response.status().is_success() => {
                    response.json::<RepositoryRuntimeStatus>().await.ok()
                }
                _ => None,
            }
        } else {
            None
        };
        if runtime.is_none() {
            partial = true;
            unreachable_node_ids.push(node_id);
        }
        items.push(AdminHistoryRepositoryItem {
            member: member.clone(),
            runtime,
        });
    }

    Ok(Json(AdminHistoryRepositoriesResponse {
        configured: true,
        partial,
        unreachable_node_ids,
        items,
    }))
}

pub(super) async fn admin_replace_history_repository_membership(
    Extension(state): Extension<AppState>,
    ApiJson(request): ApiJson<ReplaceRepositoryMembershipRequest>,
) -> Result<Json<RepositoryMembership>, ApiError> {
    let membership = repository_membership_for_nodes(&state, request.node_ids).await?;
    super::raft_write(
        &state,
        crate::state::DesiredStateCommand::ReplaceRepositoryMembership {
            membership: membership.clone(),
        },
    )
    .await?;
    Ok(Json(membership))
}

async fn repository_membership_for_nodes(
    state: &AppState,
    requested_node_ids: Vec<String>,
) -> Result<RepositoryMembership, ApiError> {
    let (current_membership, known_node_ids) = {
        let store = state.store.lock().await;
        (
            store.state().repository_membership.clone(),
            store
                .list_nodes()
                .into_iter()
                .map(|node| node.node_id)
                .collect::<std::collections::BTreeSet<_>>(),
        )
    };
    let mut members = Vec::with_capacity(requested_node_ids.len());
    for node_id in requested_node_ids {
        if !known_node_ids.contains(&node_id) {
            return Err(ApiError::invalid_request(format!(
                "repository node is not a cluster member: {node_id}"
            )));
        }
        let repository_node_id = RepositoryNodeId::try_from(node_id.clone())
            .map_err(|error| ApiError::invalid_request(error.to_string()))?;
        if let Some(member) = current_membership
            .as_ref()
            .and_then(|membership| membership.repository(&repository_node_id))
        {
            members.push(member.clone());
        } else {
            members.push(
                RepositoryMember::new(
                    derived_repository_identity(state, repository_node_id)?,
                    RepositoryCapacity::default(),
                )
                .map_err(|error| ApiError::invalid_request(error.to_string()))?,
            );
        }
    }
    RepositoryMembership::new(members).map_err(|error| ApiError::invalid_request(error.to_string()))
}

pub(super) fn derived_repository_identity(
    state: &AppState,
    node_id: RepositoryNodeId,
) -> Result<RepositoryNodeIdentity, ApiError> {
    let cluster_ca_key = state
        .cluster_ca_key_pem
        .as_deref()
        .ok_or_else(|| ApiError::conflict("repository identity material is unavailable"))?;
    let signing_key = SigningKey::from_bytes(&derived_repository_key_material(
        REPOSITORY_ED25519_KEY_CONTEXT,
        &state.cluster.cluster_id,
        node_id.as_str(),
        cluster_ca_key,
    ));
    let relay_secret = StaticSecret::from(derived_repository_key_material(
        REPOSITORY_X25519_KEY_CONTEXT,
        &state.cluster.cluster_id,
        node_id.as_str(),
        cluster_ca_key,
    ));
    RepositoryNodeIdentity::new(
        node_id,
        Ed25519PublicKey::from_bytes(signing_key.verifying_key().to_bytes())
            .map_err(|error| ApiError::internal(error.to_string()))?,
        X25519PublicKey::from_bytes(X25519DalekPublicKey::from(&relay_secret).to_bytes())
            .map_err(|error| ApiError::internal(error.to_string()))?,
    )
    .map_err(|error| ApiError::internal(error.to_string()))
}

pub(super) fn derived_repository_key_material(
    context: &[u8],
    cluster_id: &str,
    node_id: &str,
    cluster_ca_key: &str,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(context);
    hasher.update(cluster_id.as_bytes());
    hasher.update([0]);
    hasher.update(node_id.as_bytes());
    hasher.update([0]);
    hasher.update(cluster_ca_key.as_bytes());
    hasher.finalize().into()
}

pub(super) fn derived_repository_signing_key(
    state: &AppState,
    node_id: &str,
) -> Result<SigningKey, ApiError> {
    let cluster_ca_key = state
        .cluster_ca_key_pem
        .as_deref()
        .ok_or_else(|| ApiError::conflict("repository identity material is unavailable"))?;
    Ok(SigningKey::from_bytes(&derived_repository_key_material(
        REPOSITORY_ED25519_KEY_CONTEXT,
        &state.cluster.cluster_id,
        node_id,
        cluster_ca_key,
    )))
}

pub(crate) async fn local_repository_runtime_status(
    state: &AppState,
) -> Result<RepositoryRuntimeStatus, ApiError> {
    state
        .repository_replica
        .lock()
        .await
        .runtime_status(u64::try_from(Utc::now().timestamp()).unwrap_or_default())
        .map_err(repository_error)
}

fn repository_error(error: RepositoryRuntimeError) -> ApiError {
    match error {
        RepositoryRuntimeError::Query(error) => ApiError::invalid_request(error.to_string()),
        RepositoryRuntimeError::WriteStopped(_) => {
            ApiError::conflict("repository history writes are temporarily stopped")
        }
        RepositoryRuntimeError::Protocol(error) => ApiError::invalid_request(error.to_string()),
        RepositoryRuntimeError::Replica(error) => ApiError::invalid_request(error.to_string()),
        RepositoryRuntimeError::ClusterBindingMismatch => {
            ApiError::unauthorized("repository cluster binding does not match")
        }
        RepositoryRuntimeError::StateLimitExceeded => {
            ApiError::conflict("repository history capacity is exhausted")
        }
        RepositoryRuntimeError::Storage(error) => ApiError::internal(error),
    }
}

async fn ensure_ready_repository_sender(
    state: &AppState,
    internal: Option<Extension<InternalSignatureAuth>>,
) -> Result<String, ApiError> {
    let Some(Extension(internal)) = internal else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    let Some(verified) = internal.verified else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    let sender = verified.context.sender_id;
    if ready_repository_ids(state)
        .await?
        .iter()
        .any(|repository_id| repository_id == &sender)
    {
        Ok(sender)
    } else {
        Err(ApiError::unauthorized("repository sender is not ready"))
    }
}

async fn ensure_cluster_sender(
    state: &AppState,
    internal: Option<Extension<InternalSignatureAuth>>,
) -> Result<String, ApiError> {
    let Some(Extension(internal)) = internal else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    let Some(verified) = internal.verified else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    let sender = verified.context.sender_id;
    if state.store.lock().await.state().nodes.contains_key(&sender) {
        Ok(sender)
    } else {
        Err(ApiError::unauthorized(
            "relay sender is not a cluster member",
        ))
    }
}

async fn ensure_syncing_or_ready_repository_sender(
    state: &AppState,
    internal: Option<Extension<InternalSignatureAuth>>,
) -> Result<String, ApiError> {
    let Some(Extension(internal)) = internal else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    let Some(verified) = internal.verified else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    let sender = verified.context.sender_id;
    let store = state.store.lock().await;
    let Some(membership) = store.state().repository_membership.as_ref() else {
        return Err(ApiError::conflict(
            "history repository membership is not configured",
        ));
    };
    let node_id =
        crate::state::history_repository::identity::RepositoryNodeId::try_from(sender.clone())
            .map_err(|_| ApiError::unauthorized("repository sender identity is invalid"))?;
    match membership.repository(&node_id) {
        Some(member) if member.lifecycle() != &RepositoryLifecycle::Retired => Ok(sender),
        _ => Err(ApiError::unauthorized(
            "repository sender is not an active member",
        )),
    }
}

async fn ready_repository_ids(state: &AppState) -> Result<Vec<String>, ApiError> {
    let store = state.store.lock().await;
    let Some(membership) = store.state().repository_membership.as_ref() else {
        return Err(ApiError::conflict(
            "history repository membership is not configured",
        ));
    };
    let ready = membership
        .ready_members()
        .map(|member| member.node_id().as_str().to_owned())
        .collect::<Vec<_>>();
    if ready.is_empty() {
        return Err(ApiError::conflict(
            "no ready history repository is available",
        ));
    }
    Ok(ready)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::history_repository::identity::{
        Ed25519PublicKey, RepositoryNodeId, X25519PublicKey,
    };

    #[test]
    fn repository_segment_identity_must_match_authenticated_sender() {
        let identity = RepositoryNodeIdentity::new(
            RepositoryNodeId::try_from("repository-a".to_owned()).expect("node id"),
            Ed25519PublicKey::from_bytes([1; 32]).expect("signing key"),
            X25519PublicKey::from_bytes([2; 32]).expect("relay key"),
        )
        .expect("identity");
        assert!(repository_identity_matches_sender(
            &identity,
            "repository-a"
        ));
        assert!(!repository_identity_matches_sender(
            &identity,
            "repository-b"
        ));
    }

    #[test]
    fn relay_frame_must_match_its_claimed_repository_source() {
        let source = crate::history_sync::RelayKeypair::from_private_key([1; 32]);
        let other_source = crate::history_sync::RelayKeypair::from_private_key([2; 32]);
        let target = crate::history_sync::RelayKeypair::from_private_key([3; 32]);
        let frame = crate::history_sync::RelayFrame::seal(
            source,
            target.public_key(),
            [4; 12],
            b"repair batch",
            b"repository-c",
        )
        .expect("relay frame");
        assert!(relay_frame_matches_source(&frame, source));
        assert!(!relay_frame_matches_source(&frame, other_source));
    }

    #[test]
    fn sync_wire_uses_identity_below_threshold_and_zstd_only_when_smaller() {
        let small = vec![7_u8; 4 * 1024 - 1];
        let (payload_encoding, encoded) =
            crate::history_sync::encode_payload(small.clone()).expect("small wire");
        let encoding = RepositorySyncWireEncoding::from(payload_encoding);
        assert_eq!(encoding, RepositorySyncWireEncoding::Identity);
        assert_eq!(encoded, small);

        let compressible = vec![0_u8; 4 * 1024 * 2];
        let (payload_encoding, encoded) =
            crate::history_sync::encode_payload(compressible.clone()).expect("compressible wire");
        let encoding = RepositorySyncWireEncoding::from(payload_encoding);
        assert_eq!(encoding, RepositorySyncWireEncoding::Zstd);
        assert!(encoded.len() < compressible.len());
        assert_eq!(
            crate::history_sync::decode_payload(payload_encoding, &encoded, compressible.len())
                .expect("bounded decode"),
            compressible
        );

        let mut state = 0x9e37_79b9_u32;
        let incompressible = (0..4 * 1024 * 2)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                u8::try_from(state & 0xff).expect("byte")
            })
            .collect::<Vec<_>>();
        let (payload_encoding, _) =
            crate::history_sync::encode_payload(incompressible).expect("raw wire");
        let encoding = RepositorySyncWireEncoding::from(payload_encoding);
        assert_eq!(encoding, RepositorySyncWireEncoding::Identity);
    }

    #[test]
    fn sync_wire_allows_bounded_canonical_expansion_and_rejects_over_one_mib() {
        let mut state = 0x9e37_79b9_u32;
        let block = (0..1024)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                u8::try_from(state & 0xff).expect("byte")
            })
            .collect::<Vec<_>>();
        let expanded = block.repeat(300);
        assert!(expanded.len() > MAX_RESPONSE_WIRE_BYTES);
        let compressed = zstd::stream::encode_all(std::io::Cursor::new(&expanded), 1)
            .expect("compressed canonical payload");
        assert!(compressed.len() <= MAX_RESPONSE_WIRE_BYTES);
        assert_eq!(
            crate::history_sync::decode_payload(
                PayloadEncoding::ZstandardLevel1,
                &compressed,
                expanded.len(),
            )
            .expect("bounded canonical decode"),
            expanded
        );

        let over_limit = block.repeat(1025);
        let over_limit = zstd::stream::encode_all(std::io::Cursor::new(&over_limit), 1)
            .expect("compressed test payload");
        assert!(
            crate::history_sync::decode_payload(
                PayloadEncoding::ZstandardLevel1,
                &over_limit,
                1024 * 1024 + 1,
            )
            .is_err()
        );
    }
}
