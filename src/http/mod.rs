use std::{
    collections::{BTreeSet, VecDeque},
    convert::Infallible,
    sync::Arc,
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Extension, FromRequest, Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{
        IntoResponse, Redirect, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, patch, post, put},
};
use chrono::{SecondsFormat, Timelike as _, Utc};
use futures_util::{Stream, StreamExt as _, future::join_all, stream};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::{
    sync::{Mutex, mpsc},
    time::{Duration, Instant},
};

use crate::{
    admin_token::{AdminTokenHash, verify_admin_token},
    cluster_identity::JoinToken,
    cluster_metadata::ClusterMetadata,
    config::Config,
    cycle::{CycleTimeZone, current_cycle_window_at},
    domain::{
        Endpoint, EndpointKind, Grant, Node, NodeQuotaReset, QuotaResetSource, RealityDomain, User,
        UserNodeQuota, UserQuotaReset, validate_group_name,
    },
    internal_auth,
    node_runtime::{
        ComponentRuntimeStatus, LocalNodeRuntimeSnapshot, NodeRuntimeEvent, NodeRuntimeHandle,
        NodeRuntimeHistorySlot, NodeRuntimeSummary, RuntimeComponent, RuntimeStatus,
        RuntimeSummaryStatus,
    },
    protocol::{RealityServerNamesSource, VlessRealityVisionTcpEndpointMeta},
    raft::{
        app::RaftFacade,
        types::{
            ClientResponse as RaftClientResponse, NodeId as RaftNodeId, NodeMeta as RaftNodeMeta,
            raft_node_id_from_ulid,
        },
    },
    reconcile::ReconcileHandle,
    state::{DesiredStateCommand, JsonSnapshotStore, StoreError},
    subscription,
    xray_supervisor::XrayHealthHandle,
};

mod web_assets;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub store: Arc<Mutex<JsonSnapshotStore>>,
    pub reconcile: ReconcileHandle,
    pub xray_health: XrayHealthHandle,
    pub node_runtime: NodeRuntimeHandle,
    pub endpoint_probe: crate::endpoint_probe::EndpointProbeHandle,
    pub cluster: Arc<ClusterMetadata>,
    pub cluster_ca_pem: Arc<String>,
    pub cluster_ca_key_pem: Arc<Option<String>>,
    pub raft: Arc<dyn RaftFacade>,
    pub raft_rpc: Option<openraft::Raft<crate::raft::types::TypeConfig>>,
    pub version_check_cache: Arc<Mutex<VersionCheckCache>>,
    pub ops_github_repo: Arc<String>,
    pub ops_github_api_base_url: Arc<String>,
    pub ops_github_client: reqwest::Client,
}

#[derive(Debug)]
pub struct ApiError {
    code: &'static str,
    message: String,
    status: StatusCode,
    details: Map<String, Value>,
}

impl ApiError {
    fn new(code: &'static str, status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            status,
            details: Map::new(),
        }
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new("invalid_request", StatusCode::BAD_REQUEST, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("not_found", StatusCode::NOT_FOUND, message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new("unauthorized", StatusCode::UNAUTHORIZED, message)
    }

    pub fn not_implemented(message: impl Into<String>) -> Self {
        Self::new("not_implemented", StatusCode::NOT_IMPLEMENTED, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new("conflict", StatusCode::CONFLICT, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal", StatusCode::INTERNAL_SERVER_ERROR, message)
    }
}

impl From<StoreError> for ApiError {
    fn from(value: StoreError) -> Self {
        match value {
            StoreError::Domain(domain) => match domain {
                crate::domain::DomainError::MissingUser { .. }
                | crate::domain::DomainError::MissingNode { .. }
                | crate::domain::DomainError::MissingEndpoint { .. }
                | crate::domain::DomainError::MissingGrantGroup { .. } => {
                    ApiError::not_found(domain.to_string())
                }
                crate::domain::DomainError::GroupNameConflict { .. }
                | crate::domain::DomainError::GrantPairConflict { .. }
                | crate::domain::DomainError::NodeInUse { .. } => {
                    ApiError::conflict(domain.to_string())
                }
                _ => ApiError::invalid_request(domain.to_string()),
            },
            StoreError::SchemaVersionMismatch { .. } => ApiError::internal(value.to_string()),
            StoreError::Migration { .. } => ApiError::internal(value.to_string()),
            StoreError::Io(_) | StoreError::SerdeJson(_) => ApiError::internal(value.to_string()),
        }
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    error: ErrorBody,
}

#[derive(Serialize)]
struct ErrorBody {
    code: String,
    message: String,
    details: Map<String, Value>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorResponse {
            error: ErrorBody {
                code: self.code.to_string(),
                message: self.message,
                details: self.details,
            },
        };
        (self.status, Json(body)).into_response()
    }
}

pub struct ApiJson<T>(pub T);

#[axum::async_trait]
impl<S, T> FromRequest<S> for ApiJson<T>
where
    axum::Json<T>: FromRequest<S>,
    <axum::Json<T> as FromRequest<S>>::Rejection: std::fmt::Display,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = axum::Json::<T>::from_request(req, state)
            .await
            .map_err(|e| ApiError::invalid_request(e.to_string()))?;
        Ok(Self(value))
    }
}

#[derive(Serialize)]
struct Items<T> {
    items: Vec<T>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum EndpointProbeStatus {
    Missing,
    Up,
    Degraded,
    Down,
}

#[derive(Debug, Clone, Serialize)]
struct AdminEndpointProbeSlot {
    hour: String,
    status: EndpointProbeStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    checked_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency_ms_p50: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
struct AdminEndpointProbeSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    latest_checked_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    latest_latency_ms_p50: Option<u32>,
    slots: Vec<AdminEndpointProbeSlot>,
}

#[derive(Debug, Clone, Serialize)]
struct AdminEndpointWithProbe {
    #[serde(flatten)]
    endpoint: Endpoint,
    probe: AdminEndpointProbeSummary,
}

#[derive(Debug, Clone, Serialize)]
struct AdminEndpointProbeHistoryNode {
    node_id: String,
    ok: bool,
    skipped: bool,
    checked_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    config_hash: String,
}

#[derive(Debug, Clone, Serialize)]
struct AdminEndpointProbeHistorySlot {
    hour: String,
    status: EndpointProbeStatus,
    ok_count: usize,
    sample_count: usize,
    skipped_count: usize,
    tested_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency_ms_p50: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency_ms_p95: Option<u32>,
    by_node: Vec<AdminEndpointProbeHistoryNode>,
}

#[derive(Debug, Clone, Serialize)]
struct AdminEndpointProbeHistoryResponse {
    endpoint_id: String,
    expected_nodes: usize,
    slots: Vec<AdminEndpointProbeHistorySlot>,
}

#[derive(Debug, Deserialize)]
struct EndpointProbeHistoryQuery {
    hours: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AdminInternalEndpointProbeRunRequest {
    run_id: String,
    hour: String,
    config_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AdminInternalEndpointProbeRunResponse {
    accepted: bool,
    already_running: bool,
    run_id: String,
    hour: String,
}

#[derive(Debug, Serialize)]
struct AdminEndpointProbeRunNode {
    node_id: String,
    accepted: bool,
    already_running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct AdminEndpointProbeRunResponse {
    run_id: String,
    hour: String,
    config_hash: String,
    nodes: Vec<AdminEndpointProbeRunNode>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AdminEndpointProbeRunProgressStatus {
    Running,
    Finished,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AdminEndpointProbeRunProgress {
    run_id: String,
    hour: String,
    config_hash: String,
    status: AdminEndpointProbeRunProgressStatus,
    endpoints_total: usize,
    endpoints_done: usize,
    started_at: String,
    updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    finished_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AdminInternalEndpointProbeRunStatusResponse {
    requested_run_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    requested: Option<AdminEndpointProbeRunProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current: Option<AdminEndpointProbeRunProgress>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum AdminEndpointProbeRunNodeStatus {
    Running,
    Finished,
    Failed,
    Busy,
    NotFound,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum AdminEndpointProbeRunOverallStatus {
    Running,
    Finished,
    Failed,
    NotFound,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
struct AdminEndpointProbeRunStatusNode {
    node_id: String,
    status: AdminEndpointProbeRunNodeStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    progress: Option<AdminEndpointProbeRunProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current: Option<AdminEndpointProbeRunProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct AdminEndpointProbeRunStatusResponse {
    run_id: String,
    status: AdminEndpointProbeRunOverallStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    hour: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    config_hash: Option<String>,
    nodes: Vec<AdminEndpointProbeRunStatusNode>,
}

#[derive(Debug, Clone, Serialize)]
struct AdminEndpointProbeRunSseHello {
    run_id: String,
    connected_at: String,
    nodes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct AdminEndpointProbeRunSseNodeProgress {
    node_id: String,
    progress: AdminEndpointProbeRunProgress,
}

#[derive(Debug, Clone, Serialize)]
struct AdminEndpointProbeRunSseEndpointSample {
    node_id: String,
    run_id: String,
    hour: String,
    sample: crate::state::EndpointProbeAppendSample,
}

#[derive(Debug, Clone, Serialize)]
struct AdminEndpointProbeRunSseNodeError {
    node_id: String,
    run_id: String,
    error: String,
}

#[derive(Debug, Clone, Serialize)]
struct AdminEndpointProbeRunSseLagged {
    node_id: String,
    run_id: String,
    missed: u64,
}

#[derive(Debug, Clone, Serialize)]
struct AdminEndpointProbeRunSseNotFound {
    node_id: String,
    run_id: String,
}

#[derive(Serialize)]
struct ClusterInfoResponse {
    cluster_id: String,
    node_id: String,
    role: &'static str,
    leader_api_base_url: String,
    term: u64,
    xp_version: String,
}

#[derive(Deserialize)]
struct CreateJoinTokenRequest {
    ttl_seconds: i64,
}

#[derive(Serialize)]
struct CreateJoinTokenResponse {
    join_token: String,
}

#[derive(Deserialize)]
struct ClusterJoinRequest {
    join_token: String,
    node_name: String,
    #[serde(alias = "public_domain")]
    access_host: String,
    api_base_url: String,
    csr_pem: String,
}

#[derive(Serialize)]
struct ClusterJoinResponse {
    node_id: String,
    signed_cert_pem: String,
    cluster_ca_pem: String,
    cluster_ca_key_pem: String,
    xp_admin_token_hash: String,
}

#[derive(Deserialize)]
struct CreateUserRequest {
    display_name: String,
    #[serde(default)]
    quota_reset: Option<UserQuotaReset>,
}

#[derive(Deserialize)]
struct PatchNodeRequest {
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    node_name: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_string",
        alias = "public_domain"
    )]
    access_host: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    api_base_url: Option<Option<String>>,
    #[serde(default)]
    quota_limit_bytes: Option<u64>,
    #[serde(default)]
    quota_reset: Option<NodeQuotaReset>,
}

#[derive(Serialize)]
struct AdminServiceConfigResponse {
    bind: String,
    xray_api_addr: String,
    data_dir: String,
    node_name: String,
    access_host: String,
    api_base_url: String,
    quota_poll_interval_secs: u64,
    quota_auto_unban: bool,
    admin_token_present: bool,
    admin_token_masked: String,
}

#[derive(Deserialize)]
struct PatchUserRequest {
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    display_name: Option<Option<String>>,
    #[serde(default)]
    priority_tier: Option<crate::domain::UserPriorityTier>,
    #[serde(default)]
    quota_reset: Option<UserQuotaReset>,
}

#[derive(Deserialize)]
struct CreateGrantGroupMemberRequest {
    user_id: String,
    endpoint_id: String,
    enabled: bool,
    quota_limit_bytes: u64,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Deserialize)]
struct CreateGrantGroupRequest {
    group_name: String,
    members: Vec<CreateGrantGroupMemberRequest>,
}

#[derive(Deserialize)]
struct ReplaceGrantGroupRequest {
    #[serde(default)]
    rename_to: Option<String>,
    members: Vec<CreateGrantGroupMemberRequest>,
}

#[derive(Serialize)]
struct AdminGrantGroup {
    group_name: String,
}

#[derive(Serialize)]
struct AdminGrantGroupSummary {
    group_name: String,
    member_count: usize,
}

#[derive(Serialize)]
struct AdminGrantGroupMember {
    user_id: String,
    endpoint_id: String,
    enabled: bool,
    quota_limit_bytes: u64,
    note: Option<String>,
    credentials: crate::domain::GrantCredentials,
}

#[derive(Serialize)]
struct AdminGrantGroupDetail {
    group: AdminGrantGroup,
    members: Vec<AdminGrantGroupMember>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
struct RealityConfig {
    dest: String,
    server_names: Vec<String>,
    #[serde(default)]
    server_names_source: RealityServerNamesSource,
    fingerprint: String,
}

fn deserialize_optional_string<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(Some(value))
}

fn deserialize_optional_reality<'de, D>(
    deserializer: D,
) -> Result<Option<Option<RealityConfig>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<RealityConfig>::deserialize(deserializer)?;
    Ok(Some(value))
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CreateEndpointRequest {
    VlessRealityVisionTcp {
        node_id: String,
        port: u16,
        reality: RealityConfig,
    },
    #[serde(rename = "ss2022_2022_blake3_aes_128_gcm")]
    Ss2022_2022Blake3Aes128Gcm { node_id: String, port: u16 },
}

#[derive(Deserialize)]
struct PatchEndpointRequest {
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    node_id: Option<Option<String>>,
    port: Option<u16>,
    #[serde(default, deserialize_with = "deserialize_optional_reality")]
    reality: Option<Option<RealityConfig>>,
}

#[derive(Debug, Deserialize)]
struct CreateRealityDomainRequest {
    server_name: String,
    #[serde(default)]
    disabled_node_ids: BTreeSet<String>,
}

#[derive(Debug, Deserialize)]
struct PatchRealityDomainRequest {
    #[serde(default)]
    server_name: Option<String>,
    #[serde(default)]
    disabled_node_ids: Option<BTreeSet<String>>,
}

#[derive(Debug, Deserialize)]
struct ReorderRealityDomainsRequest {
    domain_ids: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
pub fn build_router(
    config: Config,
    store: Arc<Mutex<JsonSnapshotStore>>,
    reconcile: ReconcileHandle,
    xray_health: XrayHealthHandle,
    node_runtime: NodeRuntimeHandle,
    endpoint_probe: crate::endpoint_probe::EndpointProbeHandle,
    cluster: ClusterMetadata,
    cluster_ca_pem: String,
    cluster_ca_key_pem: Option<String>,
    raft: Arc<dyn RaftFacade>,
    raft_rpc: Option<openraft::Raft<crate::raft::types::TypeConfig>>,
) -> Router {
    let cluster_id = cluster.cluster_id.clone();
    let auth_state = AdminAuthState {
        admin_token_hash: config.admin_token_hash(),
        cluster_id,
        cluster_ca_key_pem: cluster_ca_key_pem.clone(),
    };

    let ops_github_repo =
        std::env::var("XP_OPS_GITHUB_REPO").unwrap_or_else(|_| "IvanLi-CN/xp".to_string());
    let ops_github_api_base_url = std::env::var("XP_OPS_GITHUB_API_BASE_URL")
        .unwrap_or_else(|_| "https://api.github.com".to_string());
    let ops_github_client = reqwest::Client::builder()
        .user_agent(format!("xp/{}", crate::version::VERSION))
        .build()
        .expect("build reqwest client");

    let app_state = AppState {
        config: Arc::new(config),
        store,
        reconcile,
        xray_health,
        node_runtime,
        endpoint_probe,
        cluster: Arc::new(cluster),
        cluster_ca_pem: Arc::new(cluster_ca_pem),
        cluster_ca_key_pem: Arc::new(cluster_ca_key_pem),
        raft,
        raft_rpc: raft_rpc.clone(),
        version_check_cache: Arc::new(Mutex::new(VersionCheckCache { entry: None })),
        ops_github_repo: Arc::new(ops_github_repo),
        ops_github_api_base_url: Arc::new(ops_github_api_base_url),
        ops_github_client,
    };

    let admin = Router::new()
        .route(
            "/_internal/raft/client-write",
            post(admin_internal_raft_client_write),
        )
        .route(
            "/_internal/raft/change-membership",
            post(admin_internal_raft_change_membership),
        )
        .route(
            "/_internal/raft/set-nodes",
            post(admin_internal_raft_set_nodes),
        )
        .route("/cluster/join-tokens", post(admin_create_join_token))
        .route("/config", get(admin_get_config))
        .route("/nodes", get(admin_list_nodes))
        .route("/nodes/runtime", get(admin_list_nodes_runtime))
        .route("/nodes/:node_id/runtime", get(admin_get_node_runtime))
        .route(
            "/nodes/:node_id/runtime/events",
            get(admin_stream_node_runtime_events),
        )
        .route(
            "/nodes/:node_id",
            get(admin_get_node)
                .patch(admin_patch_node)
                .delete(admin_delete_node),
        )
        .route(
            "/endpoints",
            post(admin_create_endpoint).get(admin_list_endpoints),
        )
        .route(
            "/endpoints/:endpoint_id",
            get(admin_get_endpoint)
                .delete(admin_delete_endpoint)
                .patch(admin_patch_endpoint),
        )
        .route(
            "/endpoints/:endpoint_id/rotate-shortid",
            post(admin_rotate_short_id),
        )
        .route(
            "/reality-domains",
            get(admin_list_reality_domains).post(admin_create_reality_domain),
        )
        .route(
            "/reality-domains/reorder",
            post(admin_reorder_reality_domains),
        )
        .route(
            "/reality-domains/:domain_id",
            patch(admin_patch_reality_domain).delete(admin_delete_reality_domain),
        )
        .route("/endpoints/probe/run", post(admin_run_endpoint_probe_run))
        .route(
            "/endpoints/probe/runs/:run_id",
            get(admin_get_endpoint_probe_run_status),
        )
        .route(
            "/endpoints/probe/runs/:run_id/events",
            get(admin_stream_endpoint_probe_run_events),
        )
        .route(
            "/endpoints/:endpoint_id/probe-history",
            get(admin_get_endpoint_probe_history),
        )
        .route("/users", post(admin_create_user).get(admin_list_users))
        .route(
            "/users/quota-summaries",
            get(admin_list_user_quota_summaries),
        )
        .route(
            "/users/:user_id",
            get(admin_get_user)
                .delete(admin_delete_user)
                .patch(admin_patch_user),
        )
        .route("/users/:user_id/reset-token", post(admin_reset_user_token))
        .route(
            "/users/:user_id/node-quotas/status",
            get(admin_get_user_node_quota_status),
        )
        .route(
            "/users/:user_id/node-quotas",
            get(admin_list_user_node_quotas),
        )
        .route(
            "/users/:user_id/node-quotas/:node_id",
            put(admin_put_user_node_quota),
        )
        .route(
            "/users/:user_id/node-weights",
            get(admin_list_user_node_weights),
        )
        .route(
            "/users/:user_id/node-weights/:node_id",
            put(admin_put_user_node_weight),
        )
        .route(
            "/grant-groups",
            get(admin_list_grant_groups).post(admin_create_grant_group),
        )
        .route(
            "/grant-groups/:group_name",
            get(admin_get_grant_group)
                .put(admin_replace_grant_group)
                .delete(admin_delete_grant_group),
        )
        .route(
            "/_internal/endpoint-probe/run",
            post(admin_internal_endpoint_probe_run),
        )
        .route(
            "/_internal/endpoint-probe/runs/:run_id",
            get(admin_internal_endpoint_probe_run_status),
        )
        .route(
            "/_internal/endpoint-probe/runs/:run_id/events",
            get(admin_internal_endpoint_probe_run_events),
        )
        .route(
            "/_internal/nodes/runtime/local",
            get(admin_internal_get_local_node_runtime),
        )
        .route(
            "/_internal/nodes/runtime/local/events",
            get(admin_internal_stream_local_node_runtime_events),
        )
        .route("/alerts", get(admin_get_alerts))
        .layer(middleware::from_fn_with_state(auth_state, admin_auth));

    let api = Router::new()
        .route("/health", get(health))
        .route("/cluster/info", get(cluster_info))
        .route("/version/check", get(api_version_check))
        .route("/cluster/join", post(cluster_join))
        .route("/sub/:subscription_token", get(get_subscription))
        .nest("/admin", admin)
        .fallback(fallback_not_found);

    let mut app = Router::new()
        .nest("/api", api)
        .route("/assets/*path", get(embedded_asset))
        .fallback(embedded_spa_fallback);

    if let Some(raft) = raft_rpc {
        app = app.merge(crate::raft::http_rpc::build_raft_rpc_router(
            crate::raft::http_rpc::RaftRpcState { raft },
        ));
    }

    app.layer(middleware::from_fn(redirect_follower_writes))
        .layer(Extension(app_state))
}

async fn admin_auth(
    State(auth): State<AdminAuthState>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    if let (Some(sig), Some(ca_key_pem)) = (
        extract_internal_signature(req.headers()),
        auth.cluster_ca_key_pem.as_deref(),
    ) && internal_auth::verify_request(ca_key_pem, req.method(), req.uri(), &sig)
    {
        // Mark the request so handlers can distinguish internal-signed calls from bearer-token calls.
        req.extensions_mut().insert(InternalSignatureAuth);
        return next.run(req).await;
    }

    let Some(token) = extract_bearer_token(req.headers()) else {
        return ApiError::unauthorized("missing or invalid authorization token").into_response();
    };
    let Some(expected) = auth.admin_token_hash.as_ref() else {
        return ApiError::unauthorized("missing or invalid authorization token").into_response();
    };

    if verify_admin_token(&token, expected) {
        return next.run(req).await;
    }
    if crate::login_token::decode_and_validate_login_token_jwt(
        &token,
        Utc::now(),
        expected.as_str(),
        &auth.cluster_id,
    )
    .is_ok()
    {
        return next.run(req).await;
    }

    ApiError::unauthorized("missing or invalid authorization token").into_response()
}

#[derive(Clone, Copy)]
struct InternalSignatureAuth;

fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::AUTHORIZATION)?;
    let raw = raw.to_str().ok()?;
    let raw = raw.strip_prefix("Bearer ")?;
    Some(raw.to_string())
}

fn extract_internal_signature(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::HeaderName::from_static(
        internal_auth::INTERNAL_SIGNATURE_HEADER,
    ))?;
    raw.to_str().ok().map(|s| s.to_string())
}

#[derive(Clone)]
struct AdminAuthState {
    admin_token_hash: Option<AdminTokenHash>,
    cluster_id: String,
    cluster_ca_key_pem: Option<String>,
}

async fn health(Extension(state): Extension<AppState>) -> Json<serde_json::Value> {
    let snap = state.xray_health.snapshot().await;
    Json(json!({
        "status": "ok",
        "xray": {
            "status": snap.status.as_str(),
            "last_ok_at": snap.last_ok_at.map(|t| t.to_rfc3339()),
            "last_fail_at": snap.last_fail_at.map(|t| t.to_rfc3339()),
            "down_since": snap.down_since.map(|t| t.to_rfc3339()),
            "consecutive_failures": snap.consecutive_failures,
            "recoveries_observed": snap.recoveries_observed,
        }
    }))
}

fn raft_metrics(state: &AppState) -> openraft::RaftMetrics<RaftNodeId, RaftNodeMeta> {
    state.raft.metrics().borrow().clone()
}

fn is_leader(metrics: &openraft::RaftMetrics<RaftNodeId, RaftNodeMeta>) -> bool {
    matches!(metrics.state, openraft::ServerState::Leader)
}

fn leader_api_base_url(
    metrics: &openraft::RaftMetrics<RaftNodeId, RaftNodeMeta>,
) -> Option<String> {
    let leader_id = metrics.current_leader?;
    metrics
        .membership_config
        .nodes()
        .find(|(id, _node)| **id == leader_id)
        .map(|(_id, node)| node.api_base_url.clone())
}

async fn raft_write(
    state: &AppState,
    cmd: crate::state::DesiredStateCommand,
) -> Result<crate::state::DesiredStateApplyResult, ApiError> {
    let resp = state
        .raft
        .client_write(cmd)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    match resp {
        RaftClientResponse::Ok { result } => Ok(result),
        RaftClientResponse::Err {
            status,
            code,
            message,
        } => {
            let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            let code_static = match code.as_str() {
                "invalid_request" => "invalid_request",
                "not_found" => "not_found",
                "conflict" => "conflict",
                "unauthorized" => "unauthorized",
                _ => "internal",
            };
            Err(ApiError::new(code_static, status, message))
        }
    }
}

async fn redirect_follower_writes(req: Request<Body>, next: Next) -> Response {
    use axum::http::Method;

    let method = req.method().clone();
    let path = req.uri().path();
    let is_write = matches!(
        method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    );

    let is_cluster_write = path == "/api/cluster/join";

    if !is_write || !is_cluster_write {
        return next.run(req).await;
    }

    let Some(state) = req.extensions().get::<AppState>() else {
        return ApiError::internal("missing AppState extension").into_response();
    };
    let metrics = raft_metrics(state);
    if is_leader(&metrics) {
        return next.run(req).await;
    }

    let Some(leader_base_url) = leader_api_base_url(&metrics) else {
        return ApiError::internal("no leader available").into_response();
    };

    let suffix = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or(path);
    let location = format!("{}{}", leader_base_url.trim_end_matches('/'), suffix);
    Redirect::temporary(&location).into_response()
}

async fn cluster_info(
    Extension(state): Extension<AppState>,
) -> Result<Json<ClusterInfoResponse>, ApiError> {
    let metrics = raft_metrics(&state);
    let leader_api_base_url = leader_api_base_url(&metrics).unwrap_or_default();
    let role = if is_leader(&metrics) {
        "leader"
    } else {
        "follower"
    };
    Ok(Json(ClusterInfoResponse {
        cluster_id: state.cluster.cluster_id.clone(),
        node_id: state.cluster.node_id.clone(),
        role,
        leader_api_base_url,
        term: metrics.current_term,
        xp_version: crate::version::VERSION.to_string(),
    }))
}

#[derive(Clone)]
pub struct VersionCheckCache {
    entry: Option<VersionCheckCacheEntry>,
}

#[derive(Clone)]
struct VersionCheckCacheEntry {
    fetched_at: Instant,
    checked_at: String,
    latest_release_tag: String,
    latest_published_at: Option<String>,
}

const VERSION_CHECK_TTL: Duration = Duration::from_secs(60 * 60);

#[derive(Serialize)]
struct VersionCheckResponse {
    current: VersionCheckCurrent,
    latest: VersionCheckLatest,
    has_update: Option<bool>,
    checked_at: String,
    compare_reason: VersionCheckCompareReason,
    source: VersionCheckSource,
}

#[derive(Serialize)]
struct VersionCheckCurrent {
    package: String,
    release_tag: String,
}

#[derive(Serialize)]
struct VersionCheckLatest {
    release_tag: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    published_at: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum VersionCheckCompareReason {
    Semver,
    Uncomparable,
}

#[derive(Serialize)]
struct VersionCheckSource {
    kind: &'static str,
    repo: String,
    api_base: String,
    channel: &'static str,
}

#[derive(Deserialize)]
struct GithubLatestReleaseResponse {
    tag_name: String,
    published_at: Option<String>,
}

async fn api_version_check(
    Extension(state): Extension<AppState>,
) -> Result<Json<VersionCheckResponse>, ApiError> {
    let current_package = crate::version::VERSION.to_string();
    let current_release_tag = format!("v{current_package}");

    let cached = { state.version_check_cache.lock().await.entry.clone() };
    let (latest_release_tag, latest_published_at, checked_at) = if let Some(entry) = cached
        && entry.fetched_at.elapsed() < VERSION_CHECK_TTL
    {
        (
            entry.latest_release_tag,
            entry.latest_published_at,
            entry.checked_at,
        )
    } else {
        let (tag, published_at) = fetch_github_latest_release(&state).await?;
        let checked_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let mut cache = state.version_check_cache.lock().await;
        cache.entry = Some(VersionCheckCacheEntry {
            fetched_at: Instant::now(),
            checked_at: checked_at.clone(),
            latest_release_tag: tag.clone(),
            latest_published_at: published_at.clone(),
        });
        (tag, published_at, checked_at)
    };

    let (has_update, compare_reason) =
        compare_simple_semver(&current_release_tag, &latest_release_tag);

    Ok(Json(VersionCheckResponse {
        current: VersionCheckCurrent {
            package: current_package,
            release_tag: current_release_tag,
        },
        latest: VersionCheckLatest {
            release_tag: latest_release_tag,
            published_at: latest_published_at,
        },
        has_update,
        checked_at,
        compare_reason,
        source: VersionCheckSource {
            kind: "github-releases",
            repo: state.ops_github_repo.as_str().to_string(),
            api_base: state.ops_github_api_base_url.as_str().to_string(),
            channel: "stable",
        },
    }))
}

async fn fetch_github_latest_release(
    state: &AppState,
) -> Result<(String, Option<String>), ApiError> {
    let api_base = state.ops_github_api_base_url.trim_end_matches('/');
    let repo = state.ops_github_repo.trim().trim_matches('/');
    let url = format!("{api_base}/repos/{repo}/releases/latest");

    let resp = state
        .ops_github_client
        .get(url)
        .header(header::ACCEPT, "application/vnd.github+json")
        .send()
        .await;

    match resp {
        Ok(resp) if resp.status().is_success() => {
            let body: GithubLatestReleaseResponse = resp.json().await.map_err(|e| {
                ApiError::new("upstream_error", StatusCode::BAD_GATEWAY, e.to_string())
            })?;

            let published_at = match body.published_at {
                Some(raw) => {
                    let dt = chrono::DateTime::parse_from_rfc3339(&raw).map_err(|e| {
                        ApiError::new("upstream_error", StatusCode::BAD_GATEWAY, e.to_string())
                    })?;
                    Some(
                        dt.with_timezone(&Utc)
                            .to_rfc3339_opts(SecondsFormat::Secs, true),
                    )
                }
                None => None,
            };

            Ok((body.tag_name, published_at))
        }
        Ok(resp) => {
            // Avoid surfacing flaky GitHub API failures (rate-limits, transient 5xx) as hard UI
            // errors if we can derive the latest release tag from the HTML redirect endpoint.
            if api_base == "https://api.github.com"
                && let Ok(out) = fetch_github_latest_release_via_redirect(state, repo).await
            {
                return Ok(out);
            }

            Err(ApiError::new(
                "upstream_error",
                StatusCode::BAD_GATEWAY,
                format!("github returned status: {}", resp.status()),
            ))
        }
        Err(err) => {
            if api_base == "https://api.github.com"
                && let Ok(out) = fetch_github_latest_release_via_redirect(state, repo).await
            {
                return Ok(out);
            }

            Err(ApiError::new(
                "upstream_error",
                StatusCode::BAD_GATEWAY,
                err.to_string(),
            ))
        }
    }
}

async fn fetch_github_latest_release_via_redirect(
    state: &AppState,
    repo: &str,
) -> Result<(String, Option<String>), ApiError> {
    let repo = repo.trim().trim_matches('/');
    if repo.is_empty() {
        return Err(ApiError::invalid_request("github repo is required"));
    }

    let url = format!("https://github.com/{repo}/releases/latest");
    let resp = state
        .ops_github_client
        .get(url)
        .header(header::ACCEPT, "text/html")
        .send()
        .await
        .map_err(|e| ApiError::new("upstream_error", StatusCode::BAD_GATEWAY, e.to_string()))?;

    if !resp.status().is_success() {
        return Err(ApiError::new(
            "upstream_error",
            StatusCode::BAD_GATEWAY,
            format!("github releases/latest returned status: {}", resp.status()),
        ));
    }

    let Some(tag) = github_release_tag_from_url(resp.url()) else {
        return Err(ApiError::new(
            "upstream_error",
            StatusCode::BAD_GATEWAY,
            "github releases/latest returned unexpected url".to_string(),
        ));
    };

    // Redirect-based lookup does not expose published_at without fetching the API JSON.
    Ok((tag, None))
}

fn github_release_tag_from_url(url: &reqwest::Url) -> Option<String> {
    let segments: Vec<_> = url.path_segments()?.collect();
    let idx = segments.iter().position(|s| *s == "tag")?;
    let tag = segments.get(idx + 1)?;
    if tag.trim().is_empty() {
        return None;
    }
    Some(tag.to_string())
}

fn compare_simple_semver(current: &str, latest: &str) -> (Option<bool>, VersionCheckCompareReason) {
    let Some(current) = parse_simple_semver(current) else {
        return (None, VersionCheckCompareReason::Uncomparable);
    };
    let Some(latest) = parse_simple_semver(latest) else {
        return (None, VersionCheckCompareReason::Uncomparable);
    };

    (Some(latest > current), VersionCheckCompareReason::Semver)
}

fn parse_simple_semver(raw: &str) -> Option<(u64, u64, u64)> {
    let raw = raw.trim();
    let raw = raw
        .strip_prefix('v')
        .or_else(|| raw.strip_prefix('V'))
        .unwrap_or(raw);
    let core = raw.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major: u64 = parts.next()?.parse().ok()?;
    let minor: u64 = parts.next()?.parse().ok()?;
    let patch: u64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn validate_https_origin(origin: &str) -> Result<(), ApiError> {
    let url = reqwest::Url::parse(origin)
        .map_err(|_| ApiError::invalid_request("api_base_url must be a valid URL"))?;
    if url.scheme() != "https" {
        return Err(ApiError::invalid_request("api_base_url must use https"));
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(ApiError::invalid_request(
            "api_base_url must be an origin (no path/query)",
        ));
    }
    Ok(())
}

async fn admin_internal_raft_client_write(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(cmd): ApiJson<DesiredStateCommand>,
) -> Result<Json<RaftClientResponse>, ApiError> {
    if internal.is_none() {
        return Err(ApiError::unauthorized("internal auth required"));
    }
    let resp = state
        .raft
        .client_write(cmd)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(resp))
}

#[derive(Deserialize)]
struct InternalChangeMembershipRequest {
    retain: bool,
    changes: InternalChangeMembers,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum InternalChangeMembers {
    RemoveVoters { node_ids: Vec<RaftNodeId> },
    RemoveNodes { node_ids: Vec<RaftNodeId> },
}

async fn admin_internal_raft_change_membership(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(req): ApiJson<InternalChangeMembershipRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if internal.is_none() {
        return Err(ApiError::unauthorized("internal auth required"));
    }

    let node_ids: BTreeSet<RaftNodeId> = match &req.changes {
        InternalChangeMembers::RemoveVoters { node_ids } => node_ids.iter().cloned().collect(),
        InternalChangeMembers::RemoveNodes { node_ids } => node_ids.iter().cloned().collect(),
    };
    if node_ids.is_empty() {
        return Err(ApiError::invalid_request("node_ids is empty"));
    }

    let metrics = raft_metrics(&state);
    if !is_leader(&metrics) {
        return Err(ApiError::invalid_request("not leader"));
    }

    let Some(raft) = state.raft_rpc.clone() else {
        return Err(ApiError::not_implemented("raft rpc is not available"));
    };

    let changes = match req.changes {
        InternalChangeMembers::RemoveVoters { .. } => {
            openraft::ChangeMembers::RemoveVoters(node_ids)
        }
        InternalChangeMembers::RemoveNodes { .. } => openraft::ChangeMembers::RemoveNodes(node_ids),
    };

    raft.change_membership(changes, req.retain)
        .await
        .map_err(|e| ApiError::internal(format!("change_membership: {e}")))?;

    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct InternalSetNodesRequest {
    nodes: Vec<InternalSetNode>,
}

#[derive(Deserialize)]
struct InternalSetNode {
    node_id: String,
    node_name: String,
    api_base_url: String,
}

async fn admin_internal_raft_set_nodes(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(req): ApiJson<InternalSetNodesRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if internal.is_none() {
        return Err(ApiError::unauthorized("internal auth required"));
    }
    if req.nodes.is_empty() {
        return Err(ApiError::invalid_request("nodes is empty"));
    }

    let metrics = raft_metrics(&state);
    if !is_leader(&metrics) {
        return Err(ApiError::invalid_request("not leader"));
    }

    let Some(raft) = state.raft_rpc.clone() else {
        return Err(ApiError::not_implemented("raft rpc is not available"));
    };

    let mut map = std::collections::BTreeMap::new();
    for n in req.nodes {
        if n.node_id.trim().is_empty() {
            return Err(ApiError::invalid_request("node_id is empty"));
        }
        if n.node_name.trim().is_empty() {
            return Err(ApiError::invalid_request("node_name is empty"));
        }
        validate_https_origin(&n.api_base_url)?;

        let raft_node_id = raft_node_id_from_ulid(&n.node_id)
            .map_err(|e| ApiError::invalid_request(e.to_string()))?;
        let exists = metrics
            .membership_config
            .nodes()
            .any(|(id, _node)| *id == raft_node_id);
        if !exists {
            return Err(ApiError::invalid_request(format!(
                "node is not in membership: {}",
                n.node_id
            )));
        }

        if map.contains_key(&raft_node_id) {
            return Err(ApiError::invalid_request(format!(
                "duplicate node_id: {}",
                n.node_id
            )));
        }

        map.insert(
            raft_node_id,
            RaftNodeMeta {
                name: n.node_name,
                api_base_url: n.api_base_url.clone(),
                raft_endpoint: n.api_base_url,
            },
        );
    }

    raft.change_membership(openraft::ChangeMembers::SetNodes(map), true)
        .await
        .map_err(|e| ApiError::internal(format!("change_membership set_nodes: {e}")))?;

    Ok(Json(json!({ "ok": true })))
}

async fn admin_create_join_token(
    Extension(state): Extension<AppState>,
    ApiJson(req): ApiJson<CreateJoinTokenRequest>,
) -> Result<Json<CreateJoinTokenResponse>, ApiError> {
    let metrics = raft_metrics(&state);
    if !is_leader(&metrics) {
        return Err(ApiError::invalid_request("not leader"));
    }

    let ca_key_pem = state
        .cluster_ca_key_pem
        .as_ref()
        .clone()
        .ok_or_else(|| ApiError::internal("cluster ca key is not available on this node"))?;

    let token = JoinToken::issue_signed_at(
        state.cluster.cluster_id.clone(),
        state.config.api_base_url.clone(),
        state.cluster_ca_pem.as_str(),
        req.ttl_seconds,
        Utc::now(),
        &ca_key_pem,
    );
    Ok(Json(CreateJoinTokenResponse {
        join_token: token.encode_base64url_json(),
    }))
}

async fn cluster_join(
    Extension(state): Extension<AppState>,
    ApiJson(req): ApiJson<ClusterJoinRequest>,
) -> Result<Json<ClusterJoinResponse>, ApiError> {
    let metrics = raft_metrics(&state);
    if !is_leader(&metrics) {
        return Err(ApiError::invalid_request("not leader"));
    }

    let token = JoinToken::decode_and_validate(&req.join_token, Utc::now())
        .map_err(|e| ApiError::invalid_request(e.to_string()))?;

    let ca_key_pem = state
        .cluster_ca_key_pem
        .as_ref()
        .clone()
        .ok_or_else(|| ApiError::internal("cluster ca key is not available on this node"))?;

    token
        .validate_one_time_secret(&ca_key_pem)
        .map_err(|e| ApiError::invalid_request(e.to_string()))?;
    if token.cluster_id != state.cluster.cluster_id {
        return Err(ApiError::invalid_request("join token cluster_id mismatch"));
    }

    let node_id = token.token_id.clone();
    {
        let store = state.store.lock().await;
        if store.get_node(&node_id).is_some() {
            return Err(ApiError::invalid_request("join token already used"));
        }
    }

    // Ensure the current leader exists in the Raft state machine so joiners can replicate the
    // full node list (including the bootstrap node).
    let leader_node = {
        let store = state.store.lock().await;
        store
            .get_node(&state.cluster.node_id)
            .unwrap_or_else(|| Node {
                node_id: state.cluster.node_id.clone(),
                node_name: state.config.node_name.clone(),
                access_host: state.config.access_host.clone(),
                api_base_url: state.config.api_base_url.clone(),
                quota_limit_bytes: 0,
                quota_reset: NodeQuotaReset::default(),
            })
    };
    let _ = raft_write(
        &state,
        crate::state::DesiredStateCommand::UpsertNode {
            node: leader_node.clone(),
        },
    )
    .await?;

    let signed_cert_pem = crate::cluster_identity::sign_node_csr(
        &state.cluster.cluster_id,
        &ca_key_pem,
        &req.csr_pem,
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let node = Node {
        node_id: node_id.clone(),
        node_name: req.node_name.clone(),
        access_host: req.access_host.clone(),
        api_base_url: req.api_base_url.clone(),
        quota_limit_bytes: 0,
        quota_reset: NodeQuotaReset::default(),
    };

    let raft_node_id =
        raft_node_id_from_ulid(&node_id).map_err(|e| ApiError::invalid_request(e.to_string()))?;
    state
        .raft
        .add_learner(
            raft_node_id,
            RaftNodeMeta {
                name: node.node_name.clone(),
                api_base_url: node.api_base_url.clone(),
                raft_endpoint: node.api_base_url.clone(),
            },
        )
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let _ = raft_write(
        &state,
        crate::state::DesiredStateCommand::UpsertNode { node },
    )
    .await?;

    let join_required_log_index = raft_metrics(&state).last_log_index.unwrap_or(0);
    let expected_leader = metrics.id;
    let promotion_raft = state.raft.clone();
    let promotion_metrics = state.raft.metrics();
    tokio::spawn(async move {
        if let Err(err) = promote_joined_learner_to_voter(
            promotion_raft,
            promotion_metrics,
            expected_leader,
            raft_node_id,
            join_required_log_index,
            Duration::from_secs(30),
        )
        .await
        {
            tracing::warn!(
                raft_node_id = raft_node_id,
                expected_leader = expected_leader,
                error = %err,
                "join: voter promotion skipped"
            );
        }
    });

    Ok(Json(ClusterJoinResponse {
        node_id,
        signed_cert_pem,
        cluster_ca_pem: (*state.cluster_ca_pem).clone(),
        cluster_ca_key_pem: ca_key_pem,
        xp_admin_token_hash: state.config.admin_token_hash.clone(),
    }))
}

async fn promote_joined_learner_to_voter(
    raft: Arc<dyn RaftFacade>,
    mut metrics: tokio::sync::watch::Receiver<openraft::RaftMetrics<RaftNodeId, RaftNodeMeta>>,
    expected_leader: RaftNodeId,
    raft_node_id: RaftNodeId,
    required_log_index: u64,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        {
            let m = metrics.borrow();

            if m.state != openraft::ServerState::Leader || m.current_leader != Some(expected_leader)
            {
                return Err("leadership changed".to_string());
            }

            let membership = m.membership_config.membership();
            if membership
                .voter_ids()
                .any(|voter_id| voter_id == raft_node_id)
            {
                return Ok(());
            }

            if membership.get_node(&raft_node_id).is_none() {
                return Err("learner removed from membership".to_string());
            }

            let repl = match m.replication.as_ref() {
                Some(x) => x,
                None => return Err("no longer leader (no replication metrics)".to_string()),
            };

            match repl.get(&raft_node_id) {
                None => {
                    // Replication is not reported yet. Keep waiting.
                }
                Some(None) => {
                    // Learner is not reachable yet. Keep waiting.
                }
                Some(Some(log_id)) => {
                    if log_id.index >= required_log_index {
                        break;
                    }
                }
            }
        }

        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Err(format!("timeout after {}s", timeout.as_secs()));
        }
        let remaining = deadline - now;
        tokio::time::timeout(remaining, metrics.changed())
            .await
            .map_err(|_| format!("timeout after {}s", timeout.as_secs()))?
            .map_err(|_| "metrics sender dropped".to_string())?;
    }

    raft.add_voters(BTreeSet::from([raft_node_id]))
        .await
        .map_err(|e| format!("change_membership add voter: {e}"))?;

    Ok(())
}

#[derive(Debug, Deserialize)]
struct NodeRuntimeQuery {
    events_limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct AdminNodeRuntimeListItem {
    node_id: String,
    node_name: String,
    api_base_url: String,
    access_host: String,
    summary: NodeRuntimeSummary,
    components: Vec<ComponentRuntimeStatus>,
    recent_slots: Vec<NodeRuntimeHistorySlot>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AdminNodesRuntimeResponse {
    partial: bool,
    unreachable_nodes: Vec<String>,
    items: Vec<AdminNodeRuntimeListItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct AdminNodeRuntimeDetailResponse {
    node: Node,
    summary: NodeRuntimeSummary,
    components: Vec<ComponentRuntimeStatus>,
    recent_slots: Vec<NodeRuntimeHistorySlot>,
    events: Vec<NodeRuntimeEvent>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct AdminInternalNodeRuntimeLocalResponse {
    node_id: String,
    summary: NodeRuntimeSummary,
    components: Vec<ComponentRuntimeStatus>,
    recent_slots: Vec<NodeRuntimeHistorySlot>,
    events: Vec<NodeRuntimeEvent>,
}

impl From<LocalNodeRuntimeSnapshot> for AdminInternalNodeRuntimeLocalResponse {
    fn from(value: LocalNodeRuntimeSnapshot) -> Self {
        Self {
            node_id: value.node_id,
            summary: value.summary,
            components: value.components,
            recent_slots: value.recent_slots,
            events: value.events,
        }
    }
}

#[derive(Debug, Serialize)]
struct AdminNodeRuntimeSseHello {
    node_id: String,
    connected_at: String,
}

#[derive(Debug, Serialize)]
struct AdminNodeRuntimeSseNodeError {
    node_id: String,
    error: String,
}

#[derive(Debug, Serialize)]
struct AdminNodeRuntimeSseLagged {
    node_id: String,
    missed: u64,
}

fn runtime_event_limit(query: &NodeRuntimeQuery) -> usize {
    query.events_limit.unwrap_or(200).clamp(0, 1000) as usize
}

fn runtime_slots_unknown() -> Vec<NodeRuntimeHistorySlot> {
    let now = Utc::now();
    let base = now
        .with_minute(if now.minute() < 30 { 0 } else { 30 })
        .and_then(|v| v.with_second(0))
        .and_then(|v| v.with_nanosecond(0))
        .unwrap_or(now);
    let mut slots = Vec::with_capacity(7 * 24 * 2);
    for i in (0..(7 * 24 * 2)).rev() {
        let at = base - chrono::Duration::minutes((i as i64) * 30);
        slots.push(NodeRuntimeHistorySlot {
            slot_start: at.to_rfc3339_opts(SecondsFormat::Secs, true),
            status: RuntimeSummaryStatus::Unknown,
        });
    }
    slots
}

fn runtime_components_unknown() -> Vec<ComponentRuntimeStatus> {
    let mut components = Vec::new();
    for component in [
        RuntimeComponent::Xp,
        RuntimeComponent::Xray,
        RuntimeComponent::Cloudflared,
    ] {
        components.push(ComponentRuntimeStatus {
            component,
            status: RuntimeStatus::Unknown,
            last_ok_at: None,
            last_fail_at: None,
            down_since: None,
            consecutive_failures: 0,
            recoveries_observed: 0,
            restart_attempts: 0,
            last_restart_at: None,
            last_restart_fail_at: None,
        });
    }
    components
}

fn node_runtime_list_item_from_snapshot(
    node: &Node,
    snapshot: AdminInternalNodeRuntimeLocalResponse,
) -> AdminNodeRuntimeListItem {
    AdminNodeRuntimeListItem {
        node_id: node.node_id.clone(),
        node_name: node.node_name.clone(),
        api_base_url: node.api_base_url.clone(),
        access_host: node.access_host.clone(),
        summary: snapshot.summary,
        components: snapshot.components,
        recent_slots: snapshot.recent_slots,
    }
}

fn node_runtime_list_item_unreachable(node: &Node) -> AdminNodeRuntimeListItem {
    AdminNodeRuntimeListItem {
        node_id: node.node_id.clone(),
        node_name: node.node_name.clone(),
        api_base_url: node.api_base_url.clone(),
        access_host: node.access_host.clone(),
        summary: NodeRuntimeSummary {
            status: RuntimeSummaryStatus::Unknown,
            updated_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        },
        components: runtime_components_unknown(),
        recent_slots: runtime_slots_unknown(),
    }
}

fn node_runtime_detail_from_snapshot(
    node: Node,
    snapshot: AdminInternalNodeRuntimeLocalResponse,
) -> AdminNodeRuntimeDetailResponse {
    AdminNodeRuntimeDetailResponse {
        node,
        summary: snapshot.summary,
        components: snapshot.components,
        recent_slots: snapshot.recent_slots,
        events: snapshot.events,
    }
}

async fn admin_list_nodes_runtime(
    Extension(state): Extension<AppState>,
) -> Result<Json<AdminNodesRuntimeResponse>, ApiError> {
    let nodes = {
        let store = state.store.lock().await;
        store.list_nodes()
    };
    let local_node_id = state.cluster.node_id.clone();

    let client = build_cluster_http_client(&state)?;
    let ca_key_pem = state
        .cluster_ca_key_pem
        .as_ref()
        .as_ref()
        .ok_or_else(|| ApiError::internal("cluster ca key is not available on this node"))?;
    let uri: axum::http::Uri = "/_internal/nodes/runtime/local?events_limit=0"
        .parse()
        .expect("valid uri");
    let sig = internal_auth::sign_request(ca_key_pem, &Method::GET, &uri)
        .map_err(|e| ApiError::internal(format!("sign internal request: {e}")))?;

    let mut items = Vec::new();
    let mut unreachable_nodes = Vec::new();

    for node in nodes {
        if node.node_id == local_node_id {
            let local = state.node_runtime.snapshot(0).await;
            items.push(node_runtime_list_item_from_snapshot(&node, local.into()));
            continue;
        }

        let base = node.api_base_url.trim_end_matches('/');
        if base.is_empty() {
            unreachable_nodes.push(node.node_id.clone());
            items.push(node_runtime_list_item_unreachable(&node));
            continue;
        }

        let request = client
            .get(format!(
                "{base}/api/admin/_internal/nodes/runtime/local?events_limit=0"
            ))
            .header(
                header::HeaderName::from_static(internal_auth::INTERNAL_SIGNATURE_HEADER),
                sig.clone(),
            )
            .send();

        let response = tokio::time::timeout(Duration::from_secs(3), request).await;
        let response = match response {
            Ok(Ok(response)) => response,
            _ => {
                unreachable_nodes.push(node.node_id.clone());
                items.push(node_runtime_list_item_unreachable(&node));
                continue;
            }
        };

        if !response.status().is_success() {
            unreachable_nodes.push(node.node_id.clone());
            items.push(node_runtime_list_item_unreachable(&node));
            continue;
        }

        match response
            .json::<AdminInternalNodeRuntimeLocalResponse>()
            .await
        {
            Ok(remote) => items.push(node_runtime_list_item_from_snapshot(&node, remote)),
            Err(_) => {
                unreachable_nodes.push(node.node_id.clone());
                items.push(node_runtime_list_item_unreachable(&node));
            }
        }
    }

    items.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    unreachable_nodes.sort();
    unreachable_nodes.dedup();

    Ok(Json(AdminNodesRuntimeResponse {
        partial: !unreachable_nodes.is_empty(),
        unreachable_nodes,
        items,
    }))
}

async fn admin_get_node_runtime(
    Extension(state): Extension<AppState>,
    Path(node_id): Path<String>,
    Query(query): Query<NodeRuntimeQuery>,
) -> Result<Json<AdminNodeRuntimeDetailResponse>, ApiError> {
    let event_limit = runtime_event_limit(&query);
    let node = {
        let store = state.store.lock().await;
        store
            .get_node(&node_id)
            .ok_or_else(|| ApiError::not_found(format!("node not found: {node_id}")))?
    };

    if node.node_id == state.cluster.node_id {
        let local = state.node_runtime.snapshot(event_limit).await;
        return Ok(Json(node_runtime_detail_from_snapshot(node, local.into())));
    }

    let base = node.api_base_url.trim_end_matches('/');
    if base.is_empty() {
        return Err(ApiError::internal(format!(
            "node is unreachable: {}",
            node.node_id
        )));
    }

    let client = build_cluster_http_client(&state)?;
    let ca_key_pem = state
        .cluster_ca_key_pem
        .as_ref()
        .as_ref()
        .ok_or_else(|| ApiError::internal("cluster ca key is not available on this node"))?;
    let uri: axum::http::Uri = format!("/_internal/nodes/runtime/local?events_limit={event_limit}")
        .parse()
        .map_err(|_| ApiError::invalid_request("invalid events_limit"))?;
    let sig = internal_auth::sign_request(ca_key_pem, &Method::GET, &uri)
        .map_err(|e| ApiError::internal(format!("sign internal request: {e}")))?;

    let request = client
        .get(format!(
            "{base}/api/admin/_internal/nodes/runtime/local?events_limit={event_limit}"
        ))
        .header(
            header::HeaderName::from_static(internal_auth::INTERNAL_SIGNATURE_HEADER),
            sig,
        )
        .send();
    let response = tokio::time::timeout(Duration::from_secs(3), request)
        .await
        .map_err(|_| ApiError::internal("request timeout"))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if !response.status().is_success() {
        return Err(ApiError::internal(format!(
            "node runtime request failed: {}",
            response.status()
        )));
    }

    let snapshot = response
        .json::<AdminInternalNodeRuntimeLocalResponse>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(node_runtime_detail_from_snapshot(node, snapshot)))
}

async fn forward_local_node_runtime_events(
    handle: NodeRuntimeHandle,
    node_id: String,
    event_limit: usize,
    tx: mpsc::Sender<Event>,
) {
    let snapshot = handle.snapshot(event_limit).await;
    if tx
        .send(sse_json_event(
            "snapshot",
            &AdminInternalNodeRuntimeLocalResponse::from(snapshot),
        ))
        .await
        .is_err()
    {
        return;
    }

    let mut rx = handle.subscribe();
    loop {
        if tx.is_closed() {
            return;
        }
        match rx.recv().await {
            Ok(event) => {
                if tx.send(sse_json_event("event", &event)).await.is_err() {
                    return;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                let _ = tx
                    .send(sse_json_event(
                        "lagged",
                        &AdminNodeRuntimeSseLagged {
                            node_id: node_id.clone(),
                            missed,
                        },
                    ))
                    .await;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
        }
    }
}

async fn forward_remote_node_runtime_events(
    client: reqwest::Client,
    url: String,
    sig: String,
    node_id: String,
    tx: mpsc::Sender<Event>,
) {
    let response = match client
        .get(url)
        .header(
            header::HeaderName::from_static(internal_auth::INTERNAL_SIGNATURE_HEADER),
            sig,
        )
        .send()
        .await
    {
        Ok(response) => response,
        Err(err) => {
            let _ = tx
                .send(sse_json_event(
                    "node_error",
                    &AdminNodeRuntimeSseNodeError {
                        node_id,
                        error: err.to_string(),
                    },
                ))
                .await;
            return;
        }
    };

    if !response.status().is_success() {
        let _ = tx
            .send(sse_json_event(
                "node_error",
                &AdminNodeRuntimeSseNodeError {
                    node_id,
                    error: format!("http {}", response.status()),
                },
            ))
            .await;
        return;
    }

    let mut buffer = String::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if tx.is_closed() {
            return;
        }

        let bytes = match chunk {
            Ok(bytes) => bytes,
            Err(err) => {
                let _ = tx
                    .send(sse_json_event(
                        "node_error",
                        &AdminNodeRuntimeSseNodeError {
                            node_id,
                            error: err.to_string(),
                        },
                    ))
                    .await;
                return;
            }
        };

        let chunk_text = String::from_utf8_lossy(bytes.as_ref()).replace("\r\n", "\n");
        buffer.push_str(&chunk_text);

        while let Some(split) = buffer.find("\n\n") {
            let frame = buffer[..split].to_string();
            buffer = buffer[split + 2..].to_string();

            let mut event_type: Option<String> = None;
            let mut data_lines: Vec<&str> = Vec::new();
            for line in frame.lines() {
                let line = line.trim_end_matches('\r');
                if line.starts_with(':') {
                    continue;
                }
                if let Some(rest) = line.strip_prefix("event:") {
                    event_type = Some(rest.trim().to_string());
                    continue;
                }
                if let Some(rest) = line.strip_prefix("data:") {
                    data_lines.push(rest.trim_start());
                }
            }

            let Some(event_type) = event_type else {
                continue;
            };
            let data = data_lines.join("\n");
            if data.is_empty() {
                continue;
            }

            if tx
                .send(Event::default().event(event_type).data(data))
                .await
                .is_err()
            {
                return;
            }
        }
    }
}

async fn admin_stream_node_runtime_events(
    Extension(state): Extension<AppState>,
    Path(node_id): Path<String>,
    Query(query): Query<NodeRuntimeQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>> + Send>, ApiError> {
    let event_limit = runtime_event_limit(&query);
    let node = {
        let store = state.store.lock().await;
        store
            .get_node(&node_id)
            .ok_or_else(|| ApiError::not_found(format!("node not found: {node_id}")))?
    };

    let hello = AdminNodeRuntimeSseHello {
        node_id: node_id.clone(),
        connected_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
    };

    let mut initial_events = VecDeque::new();
    initial_events.push_back(sse_json_event("hello", &hello));

    let (tx, rx) = mpsc::channel::<Event>(512);
    if node.node_id == state.cluster.node_id {
        let handle = state.node_runtime.clone();
        tokio::spawn(async move {
            forward_local_node_runtime_events(handle, node_id, event_limit, tx).await;
        });
    } else {
        let base = node.api_base_url.trim_end_matches('/');
        if base.is_empty() {
            initial_events.push_back(sse_json_event(
                "node_error",
                &AdminNodeRuntimeSseNodeError {
                    node_id,
                    error: "node is unreachable".to_string(),
                },
            ));
        } else {
            let client = build_cluster_http_client(&state)?;
            let ca_key_pem = state.cluster_ca_key_pem.as_ref().as_ref().ok_or_else(|| {
                ApiError::internal("cluster ca key is not available on this node")
            })?;
            let uri: axum::http::Uri = "/_internal/nodes/runtime/local/events"
                .parse()
                .expect("valid uri");
            let sig = internal_auth::sign_request(ca_key_pem, &Method::GET, &uri)
                .map_err(|e| ApiError::internal(format!("sign internal request: {e}")))?;
            let url = format!("{base}/api/admin/_internal/nodes/runtime/local/events");
            tokio::spawn(async move {
                forward_remote_node_runtime_events(client, url, sig, node_id, tx).await;
            });
        }
    }

    let out_stream = stream::unfold((initial_events, rx), |(mut initial, mut rx)| async move {
        if let Some(event) = initial.pop_front() {
            return Some((Ok(event), (initial, rx)));
        }
        let next = rx.recv().await?;
        Some((Ok(next), (initial, rx)))
    });

    Ok(Sse::new(out_stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("keepalive"),
    ))
}

async fn admin_internal_get_local_node_runtime(
    Extension(state): Extension<AppState>,
    Query(query): Query<NodeRuntimeQuery>,
) -> Result<Json<AdminInternalNodeRuntimeLocalResponse>, ApiError> {
    let event_limit = runtime_event_limit(&query);
    let snapshot = state.node_runtime.snapshot(event_limit).await;
    Ok(Json(snapshot.into()))
}

async fn admin_internal_stream_local_node_runtime_events(
    Extension(state): Extension<AppState>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>> + Send>, ApiError> {
    let hello = AdminNodeRuntimeSseHello {
        node_id: state.cluster.node_id.clone(),
        connected_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
    };
    let mut initial_events = VecDeque::new();
    initial_events.push_back(sse_json_event("hello", &hello));

    let (tx, rx) = mpsc::channel::<Event>(512);
    let handle = state.node_runtime.clone();
    let node_id = state.cluster.node_id.clone();
    tokio::spawn(async move {
        forward_local_node_runtime_events(handle, node_id, 200, tx).await;
    });

    let out_stream = stream::unfold((initial_events, rx), |(mut initial, mut rx)| async move {
        if let Some(event) = initial.pop_front() {
            return Some((Ok(event), (initial, rx)));
        }
        let next = rx.recv().await?;
        Some((Ok(next), (initial, rx)))
    });

    Ok(Sse::new(out_stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("keepalive"),
    ))
}

async fn admin_list_nodes(
    Extension(state): Extension<AppState>,
) -> Result<Json<Items<Node>>, ApiError> {
    let store = state.store.lock().await;
    Ok(Json(Items {
        items: store.list_nodes(),
    }))
}

async fn admin_get_node(
    Extension(state): Extension<AppState>,
    Path(node_id): Path<String>,
) -> Result<Json<Node>, ApiError> {
    let store = state.store.lock().await;
    let node = store
        .get_node(&node_id)
        .ok_or_else(|| ApiError::not_found(format!("node not found: {node_id}")))?;
    Ok(Json(node))
}

async fn admin_patch_node(
    Extension(state): Extension<AppState>,
    Path(node_id): Path<String>,
    ApiJson(req): ApiJson<PatchNodeRequest>,
) -> Result<Json<Node>, ApiError> {
    if req.node_name.is_some() || req.access_host.is_some() || req.api_base_url.is_some() {
        return Err(ApiError::invalid_request(
            "node meta (node_name/access_host/api_base_url) is managed by xp-ops and cannot be edited via API",
        ));
    }

    let mut node = {
        let store = state.store.lock().await;
        store
            .get_node(&node_id)
            .ok_or_else(|| ApiError::not_found(format!("node not found: {node_id}")))?
    };

    if let Some(quota_reset) = req.quota_reset {
        node.quota_reset = quota_reset;
    }
    if let Some(quota_limit_bytes) = req.quota_limit_bytes {
        node.quota_limit_bytes = quota_limit_bytes;
    }

    let _ = raft_write(
        &state,
        crate::state::DesiredStateCommand::UpsertNode { node: node.clone() },
    )
    .await?;
    Ok(Json(node))
}

async fn admin_delete_node(
    Extension(state): Extension<AppState>,
    Path(node_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    if node_id == state.cluster.node_id {
        return Err(ApiError::invalid_request("cannot delete local node"));
    }

    // Preflight validation before touching Raft membership, to avoid partial updates.
    {
        let store = state.store.lock().await;
        if store.get_node(&node_id).is_none() {
            return Err(ApiError::not_found(format!("node not found: {node_id}")));
        }
        if let Some(endpoint) = store
            .list_endpoints()
            .into_iter()
            .find(|endpoint| endpoint.node_id == node_id)
        {
            return Err(ApiError::conflict(
                crate::domain::DomainError::NodeInUse {
                    node_id: node_id.clone(),
                    endpoint_id: endpoint.endpoint_id,
                }
                .to_string(),
            ));
        }
    }

    let raft_node_id =
        raft_node_id_from_ulid(&node_id).map_err(|e| ApiError::invalid_request(e.to_string()))?;

    let metrics = raft_metrics(&state);
    if metrics.current_leader == Some(raft_node_id) {
        return Err(ApiError::invalid_request("cannot delete current leader"));
    }

    let membership = metrics.membership_config.membership();
    if membership.get_node(&raft_node_id).is_some() {
        let is_voter = membership
            .voter_ids()
            .any(|voter_id| voter_id == raft_node_id);

        if is_voter {
            state
                .raft
                .change_membership(
                    openraft::ChangeMembers::RemoveVoters(BTreeSet::from([raft_node_id])),
                    true,
                )
                .await
                .map_err(|e| ApiError::internal(format!("change_membership remove_voters: {e}")))?;
        }

        state
            .raft
            .change_membership(
                openraft::ChangeMembers::RemoveNodes(BTreeSet::from([raft_node_id])),
                true,
            )
            .await
            .map_err(|e| ApiError::internal(format!("change_membership remove_nodes: {e}")))?;
    }

    let out = raft_write(
        &state,
        crate::state::DesiredStateCommand::DeleteNode {
            node_id: node_id.clone(),
        },
    )
    .await?;
    let crate::state::DesiredStateApplyResult::NodeDeleted { deleted } = out else {
        return Err(ApiError::internal("unexpected raft apply result"));
    };
    if !deleted {
        return Err(ApiError::not_found(format!("node not found: {node_id}")));
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn admin_get_config(
    Extension(state): Extension<AppState>,
) -> Result<Json<AdminServiceConfigResponse>, ApiError> {
    let admin_token_present = state.config.admin_token_hash().is_some();
    let admin_token_masked = if admin_token_present {
        "********".to_string()
    } else {
        String::new()
    };

    Ok(Json(AdminServiceConfigResponse {
        bind: state.config.bind.to_string(),
        xray_api_addr: state.config.xray_api_addr.to_string(),
        data_dir: state.config.data_dir.display().to_string(),
        node_name: state.config.node_name.clone(),
        access_host: state.config.access_host.clone(),
        api_base_url: state.config.api_base_url.clone(),
        quota_poll_interval_secs: state.config.quota_poll_interval_secs,
        quota_auto_unban: state.config.quota_auto_unban,
        admin_token_present,
        admin_token_masked,
    }))
}

async fn admin_create_endpoint(
    Extension(state): Extension<AppState>,
    ApiJson(req): ApiJson<CreateEndpointRequest>,
) -> Result<Json<Endpoint>, ApiError> {
    let (node_id, kind, port, meta) = match req {
        CreateEndpointRequest::VlessRealityVisionTcp {
            node_id,
            port,
            reality,
        } => (
            node_id,
            crate::domain::EndpointKind::VlessRealityVisionTcp,
            port,
            json!({ "reality": reality }),
        ),
        CreateEndpointRequest::Ss2022_2022Blake3Aes128Gcm { node_id, port } => (
            node_id,
            crate::domain::EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port,
            json!({}),
        ),
    };

    let endpoint = {
        let store = state.store.lock().await;
        store.build_endpoint(node_id, kind, port, meta)?
    };
    let _ = raft_write(
        &state,
        crate::state::DesiredStateCommand::UpsertEndpoint {
            endpoint: endpoint.clone(),
        },
    )
    .await?;
    state.reconcile.request_full();
    Ok(Json(endpoint))
}

async fn admin_list_endpoints(
    Extension(state): Extension<AppState>,
) -> Result<Json<Items<AdminEndpointWithProbe>>, ApiError> {
    let store = state.store.lock().await;
    let now = Utc::now();
    Ok(Json(Items {
        items: store
            .list_endpoints()
            .into_iter()
            .map(|endpoint| AdminEndpointWithProbe {
                probe: build_endpoint_probe_summary(&store, &endpoint.endpoint_id, now, 24),
                endpoint,
            })
            .collect(),
    }))
}

async fn admin_get_endpoint(
    Extension(state): Extension<AppState>,
    Path(endpoint_id): Path<String>,
) -> Result<Json<AdminEndpointWithProbe>, ApiError> {
    let store = state.store.lock().await;
    let endpoint = store
        .get_endpoint(&endpoint_id)
        .ok_or_else(|| ApiError::not_found(format!("endpoint not found: {endpoint_id}")))?;
    let now = Utc::now();
    Ok(Json(AdminEndpointWithProbe {
        probe: build_endpoint_probe_summary(&store, &endpoint.endpoint_id, now, 24),
        endpoint,
    }))
}

fn probe_status_for_counts(
    expected_nodes: usize,
    sample_count: usize,
    ok_count: usize,
    skipped_count: usize,
) -> EndpointProbeStatus {
    if expected_nodes == 0 {
        return EndpointProbeStatus::Missing;
    }
    if sample_count == 0 {
        return EndpointProbeStatus::Missing;
    }
    // If not all nodes have reported, treat this hour bucket as incomplete.
    if sample_count < expected_nodes {
        return EndpointProbeStatus::Missing;
    }
    let tested_count = sample_count.saturating_sub(skipped_count);
    if tested_count == 0 {
        // Reported, but no real tests (e.g. single-node cluster with self-test skipped).
        return EndpointProbeStatus::Missing;
    }
    if ok_count == 0 {
        return EndpointProbeStatus::Down;
    }
    if ok_count >= tested_count {
        return EndpointProbeStatus::Up;
    }
    EndpointProbeStatus::Degraded
}

#[cfg(test)]
mod endpoint_probe_status_tests {
    use super::{EndpointProbeStatus, probe_status_for_counts};

    #[test]
    fn probe_status_handles_incomplete_hours_as_missing() {
        assert_eq!(
            probe_status_for_counts(3, 2, 0, 0),
            EndpointProbeStatus::Missing
        );
        assert_eq!(
            probe_status_for_counts(3, 2, 1, 0),
            EndpointProbeStatus::Missing
        );
    }

    #[test]
    fn probe_status_down_when_all_nodes_report_and_all_fail() {
        assert_eq!(
            probe_status_for_counts(3, 3, 0, 0),
            EndpointProbeStatus::Down
        );
    }

    #[test]
    fn probe_status_up_when_all_nodes_report_and_all_ok() {
        assert_eq!(probe_status_for_counts(3, 3, 3, 0), EndpointProbeStatus::Up);
    }

    #[test]
    fn probe_status_degraded_when_mixed_ok_and_fail() {
        assert_eq!(
            probe_status_for_counts(3, 3, 2, 0),
            EndpointProbeStatus::Degraded
        );
        assert_eq!(
            probe_status_for_counts(3, 3, 1, 0),
            EndpointProbeStatus::Degraded
        );
    }

    #[test]
    fn probe_status_up_allows_skipped_samples_when_all_tested_ok() {
        assert_eq!(probe_status_for_counts(3, 3, 2, 1), EndpointProbeStatus::Up);
    }

    #[test]
    fn probe_status_missing_when_all_samples_are_skipped() {
        assert_eq!(
            probe_status_for_counts(1, 1, 0, 1),
            EndpointProbeStatus::Missing
        );
    }

    #[test]
    fn probe_status_degraded_when_tested_samples_mixed_even_with_skips() {
        assert_eq!(
            probe_status_for_counts(3, 3, 1, 1),
            EndpointProbeStatus::Degraded
        );
    }

    #[test]
    fn probe_status_down_when_all_tested_samples_fail_even_with_skips() {
        assert_eq!(
            probe_status_for_counts(3, 3, 0, 1),
            EndpointProbeStatus::Down
        );
    }
}

fn percentile_ms(sorted: &[u32], percentile: f64) -> Option<u32> {
    if sorted.is_empty() {
        return None;
    }
    let p = percentile.clamp(0.0, 1.0);
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted.get(idx).copied()
}

fn compute_latency_p50_p95_ms(samples: impl Iterator<Item = u32>) -> (Option<u32>, Option<u32>) {
    let mut values: Vec<u32> = samples.collect();
    values.sort_unstable();
    (percentile_ms(&values, 0.50), percentile_ms(&values, 0.95))
}

fn build_endpoint_probe_summary(
    store: &JsonSnapshotStore,
    endpoint_id: &str,
    now: chrono::DateTime<Utc>,
    hours: usize,
) -> AdminEndpointProbeSummary {
    let node_ids: std::collections::BTreeSet<String> =
        store.list_nodes().into_iter().map(|n| n.node_id).collect();
    let expected_nodes = node_ids.len();
    let history = store.state().endpoint_probe_history.get(endpoint_id);

    let now_hour = now
        .with_minute(0)
        .and_then(|v| v.with_second(0))
        .and_then(|v| v.with_nanosecond(0))
        .unwrap_or(now);
    let start = now_hour - chrono::Duration::hours(hours.saturating_sub(1) as i64);

    let mut slots = Vec::with_capacity(hours);
    let mut latest_checked_at: Option<String> = None;
    let mut latest_latency_ms_p50: Option<u32> = None;

    for i in 0..hours {
        let hour_dt = start + chrono::Duration::hours(i as i64);
        let hour_key = crate::endpoint_probe::format_hour_key(hour_dt);

        let Some(history) = history else {
            slots.push(AdminEndpointProbeSlot {
                hour: hour_key,
                status: EndpointProbeStatus::Missing,
                checked_at: None,
                latency_ms_p50: None,
            });
            continue;
        };

        let Some(bucket) = history.hours.get(&hour_key) else {
            slots.push(AdminEndpointProbeSlot {
                hour: hour_key,
                status: EndpointProbeStatus::Missing,
                checked_at: None,
                latency_ms_p50: None,
            });
            continue;
        };

        let sample_count = bucket
            .by_node
            .iter()
            .filter(|(node_id, _)| node_ids.contains(node_id.as_str()))
            .count();
        let skipped_count = bucket
            .by_node
            .iter()
            .filter(|(node_id, _)| node_ids.contains(node_id.as_str()))
            .map(|(_node_id, sample)| sample)
            .filter(|s| s.skipped)
            .count();
        let ok_count = bucket
            .by_node
            .iter()
            .filter(|(node_id, _)| node_ids.contains(node_id.as_str()))
            .map(|(_node_id, sample)| sample)
            .filter(|s| s.ok && !s.skipped)
            .count();
        let (p50, _p95) = compute_latency_p50_p95_ms(
            bucket
                .by_node
                .iter()
                .filter(|(node_id, _)| node_ids.contains(node_id.as_str()))
                .map(|(_node_id, sample)| sample)
                .filter(|s| s.ok && !s.skipped)
                .filter_map(|s| s.latency_ms),
        );
        let checked_at_max = bucket
            .by_node
            .iter()
            .filter(|(node_id, _)| node_ids.contains(node_id.as_str()))
            .map(|(_node_id, sample)| sample)
            .map(|s| s.checked_at.as_str())
            .max()
            .map(|s| s.to_string());

        let status = probe_status_for_counts(expected_nodes, sample_count, ok_count, skipped_count);

        // Iterate oldest -> newest. Keep the last seen as the "latest".
        let tested_count = sample_count.saturating_sub(skipped_count);
        if tested_count > 0 {
            latest_checked_at = checked_at_max.clone();
            latest_latency_ms_p50 = p50;
        }

        slots.push(AdminEndpointProbeSlot {
            hour: hour_key,
            status,
            checked_at: checked_at_max,
            latency_ms_p50: p50,
        });
    }

    AdminEndpointProbeSummary {
        latest_checked_at,
        latest_latency_ms_p50,
        slots,
    }
}

fn build_endpoint_probe_history_response(
    store: &JsonSnapshotStore,
    endpoint_id: &str,
    now: chrono::DateTime<Utc>,
    hours: usize,
) -> AdminEndpointProbeHistoryResponse {
    let node_ids: std::collections::BTreeSet<String> =
        store.list_nodes().into_iter().map(|n| n.node_id).collect();
    let expected_nodes = node_ids.len();
    let history = store.state().endpoint_probe_history.get(endpoint_id);

    let now_hour = now
        .with_minute(0)
        .and_then(|v| v.with_second(0))
        .and_then(|v| v.with_nanosecond(0))
        .unwrap_or(now);
    let start = now_hour - chrono::Duration::hours(hours.saturating_sub(1) as i64);

    let mut slots = Vec::with_capacity(hours);

    for i in 0..hours {
        let hour_dt = start + chrono::Duration::hours(i as i64);
        let hour_key = crate::endpoint_probe::format_hour_key(hour_dt);

        let bucket = history.and_then(|h| h.hours.get(&hour_key));

        let mut by_node = Vec::new();
        if let Some(bucket) = bucket {
            for (node_id, sample) in &bucket.by_node {
                if !node_ids.contains(node_id.as_str()) {
                    continue;
                }
                by_node.push(AdminEndpointProbeHistoryNode {
                    node_id: node_id.clone(),
                    ok: sample.ok,
                    skipped: sample.skipped,
                    checked_at: sample.checked_at.clone(),
                    latency_ms: sample.latency_ms,
                    target_id: sample.target_id.clone(),
                    target_url: sample.target_url.clone(),
                    error: sample.error.clone(),
                    config_hash: sample.config_hash.clone(),
                });
            }
            by_node.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        }

        let sample_count = by_node.len();
        let skipped_count = by_node.iter().filter(|s| s.skipped).count();
        let tested_count = sample_count.saturating_sub(skipped_count);
        let ok_count = by_node.iter().filter(|s| s.ok && !s.skipped).count();
        let (p50, p95) = compute_latency_p50_p95_ms(
            by_node
                .iter()
                .filter(|s| s.ok && !s.skipped)
                .filter_map(|s| s.latency_ms),
        );

        let status = probe_status_for_counts(expected_nodes, sample_count, ok_count, skipped_count);

        slots.push(AdminEndpointProbeHistorySlot {
            hour: hour_key,
            status,
            ok_count,
            sample_count,
            skipped_count,
            tested_count,
            latency_ms_p50: p50,
            latency_ms_p95: p95,
            by_node,
        });
    }

    AdminEndpointProbeHistoryResponse {
        endpoint_id: endpoint_id.to_string(),
        expected_nodes,
        slots,
    }
}

async fn admin_get_endpoint_probe_history(
    Extension(state): Extension<AppState>,
    Path(endpoint_id): Path<String>,
    Query(q): Query<EndpointProbeHistoryQuery>,
) -> Result<Json<AdminEndpointProbeHistoryResponse>, ApiError> {
    let hours = q.hours.unwrap_or(24).clamp(1, 24) as usize;
    let store = state.store.lock().await;
    if store.get_endpoint(&endpoint_id).is_none() {
        return Err(ApiError::not_found(format!(
            "endpoint not found: {endpoint_id}"
        )));
    }
    let now = Utc::now();
    Ok(Json(build_endpoint_probe_history_response(
        &store,
        &endpoint_id,
        now,
        hours,
    )))
}

fn build_cluster_http_client(state: &AppState) -> Result<reqwest::Client, ApiError> {
    let ca = reqwest::Certificate::from_pem(state.cluster_ca_pem.as_bytes())
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut builder = reqwest::Client::builder().add_root_certificate(ca);

    // Best effort: if the edge requires mTLS, attach node identity.
    let cert = state
        .cluster
        .read_node_cert_pem(&state.config.data_dir)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let key = state
        .cluster
        .read_node_key_pem(&state.config.data_dir)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let identity_pem = format!("{cert}\n{key}");
    let identity = reqwest::Identity::from_pem(identity_pem.as_bytes())
        .map_err(|e| ApiError::internal(e.to_string()))?;
    builder = builder.identity(identity);

    builder
        .build()
        .map_err(|e| ApiError::internal(format!("build cluster reqwest client: {e}")))
}

async fn admin_run_endpoint_probe_run(
    Extension(state): Extension<AppState>,
) -> Result<Json<AdminEndpointProbeRunResponse>, ApiError> {
    let run_id = crate::id::new_ulid_string();
    let now = Utc::now();
    let hour = crate::endpoint_probe::format_hour_key(now);
    let config_hash = crate::endpoint_probe::probe_config_hash();

    let nodes = {
        let store = state.store.lock().await;
        store.list_nodes()
    };

    let local_node_id = state.cluster.node_id.clone();

    let Some(ca_key_pem) = state.cluster_ca_key_pem.as_ref().as_deref() else {
        return Err(ApiError::internal("cluster ca key is not available"));
    };

    let client = build_cluster_http_client(&state)?;
    let uri: axum::http::Uri = "/_internal/endpoint-probe/run".parse().expect("valid uri");
    let sig = internal_auth::sign_request(ca_key_pem, &Method::POST, &uri)
        .map_err(|e| ApiError::internal(format!("sign internal request: {e}")))?;

    let mut tasks = Vec::new();
    for node in nodes {
        let node_id = node.node_id.clone();
        let req_body = AdminInternalEndpointProbeRunRequest {
            run_id: run_id.clone(),
            hour: hour.clone(),
            config_hash: config_hash.clone(),
        };

        if node_id == local_node_id {
            let handle = state.endpoint_probe.clone();
            tasks.push(tokio::spawn(async move {
                let out = handle
                    .start_background(crate::endpoint_probe::EndpointProbeRunRequest {
                        hour: req_body.hour.clone(),
                        run_id: req_body.run_id.clone(),
                        config_hash: req_body.config_hash.clone(),
                        reason: "manual",
                    })
                    .await;
                match out {
                    Ok(accepted) => AdminEndpointProbeRunNode {
                        node_id,
                        accepted: accepted.accepted,
                        already_running: accepted.already_running,
                        error: None,
                    },
                    Err(err) => AdminEndpointProbeRunNode {
                        node_id,
                        accepted: false,
                        already_running: false,
                        error: Some(err.to_string()),
                    },
                }
            }));
            continue;
        }

        let client = client.clone();
        let sig = sig.clone();
        let url = format!(
            "{}/api/admin/_internal/endpoint-probe/run",
            node.api_base_url.trim_end_matches('/')
        );

        tasks.push(tokio::spawn(async move {
            let request = client
                .post(url)
                .header(
                    header::HeaderName::from_static(internal_auth::INTERNAL_SIGNATURE_HEADER),
                    sig,
                )
                .json(&req_body)
                .send();

            let resp = tokio::time::timeout(Duration::from_secs(3), request).await;
            let resp = match resp {
                Ok(Ok(resp)) => resp,
                Ok(Err(err)) => {
                    return AdminEndpointProbeRunNode {
                        node_id,
                        accepted: false,
                        already_running: false,
                        error: Some(err.to_string()),
                    };
                }
                Err(_) => {
                    return AdminEndpointProbeRunNode {
                        node_id,
                        accepted: false,
                        already_running: false,
                        error: Some("timeout".to_string()),
                    };
                }
            };

            if resp.status() == StatusCode::CONFLICT {
                let body = match resp.text().await {
                    Ok(body) => body,
                    Err(err) => err.to_string(),
                };
                return AdminEndpointProbeRunNode {
                    node_id,
                    accepted: false,
                    already_running: false,
                    error: Some(if body.trim().is_empty() {
                        "conflict".to_string()
                    } else {
                        format!("conflict: {body}")
                    }),
                };
            }

            if !resp.status().is_success() {
                return AdminEndpointProbeRunNode {
                    node_id,
                    accepted: false,
                    already_running: false,
                    error: Some(format!("http {}", resp.status())),
                };
            }

            match resp.json::<AdminInternalEndpointProbeRunResponse>().await {
                Ok(out) => AdminEndpointProbeRunNode {
                    node_id,
                    accepted: out.accepted,
                    already_running: out.already_running,
                    error: None,
                },
                Err(err) => AdminEndpointProbeRunNode {
                    node_id,
                    accepted: false,
                    already_running: false,
                    error: Some(err.to_string()),
                },
            }
        }));
    }

    let mut nodes = Vec::new();
    for item in join_all(tasks).await.into_iter().flatten() {
        nodes.push(item);
    }
    nodes.sort_by(|a, b| a.node_id.cmp(&b.node_id));

    Ok(Json(AdminEndpointProbeRunResponse {
        run_id,
        hour,
        config_hash,
        nodes,
    }))
}

fn map_probe_run_snapshot(
    snapshot: crate::endpoint_probe::EndpointProbeRunSnapshot,
) -> AdminEndpointProbeRunProgress {
    let status = match snapshot.status {
        crate::endpoint_probe::EndpointProbeRunStatus::Running => {
            AdminEndpointProbeRunProgressStatus::Running
        }
        crate::endpoint_probe::EndpointProbeRunStatus::Finished => {
            AdminEndpointProbeRunProgressStatus::Finished
        }
        crate::endpoint_probe::EndpointProbeRunStatus::Failed => {
            AdminEndpointProbeRunProgressStatus::Failed
        }
    };

    AdminEndpointProbeRunProgress {
        run_id: snapshot.run_id,
        hour: snapshot.hour,
        config_hash: snapshot.config_hash,
        status,
        endpoints_total: snapshot.endpoints_total,
        endpoints_done: snapshot.endpoints_done,
        started_at: snapshot.started_at,
        updated_at: snapshot.updated_at,
        finished_at: snapshot.finished_at,
        error: snapshot.error,
    }
}

fn map_node_status(
    run_id: &str,
    node_id: String,
    requested: Option<AdminEndpointProbeRunProgress>,
    current: Option<AdminEndpointProbeRunProgress>,
    error: Option<String>,
) -> AdminEndpointProbeRunStatusNode {
    if let Some(error) = error {
        return AdminEndpointProbeRunStatusNode {
            node_id,
            status: AdminEndpointProbeRunNodeStatus::Unknown,
            progress: requested,
            current,
            error: Some(error),
        };
    }

    if let Some(progress) = requested {
        let status = match progress.status {
            AdminEndpointProbeRunProgressStatus::Running => {
                AdminEndpointProbeRunNodeStatus::Running
            }
            AdminEndpointProbeRunProgressStatus::Finished => {
                AdminEndpointProbeRunNodeStatus::Finished
            }
            AdminEndpointProbeRunProgressStatus::Failed => AdminEndpointProbeRunNodeStatus::Failed,
        };
        return AdminEndpointProbeRunStatusNode {
            node_id,
            status,
            progress: Some(progress),
            current,
            error: None,
        };
    }

    if let Some(current) = current {
        if current.run_id != run_id {
            return AdminEndpointProbeRunStatusNode {
                node_id,
                status: AdminEndpointProbeRunNodeStatus::Busy,
                progress: None,
                current: Some(current),
                error: None,
            };
        }

        // Requested run is the current run, but snapshot lookup failed for some reason.
        return AdminEndpointProbeRunStatusNode {
            node_id,
            status: AdminEndpointProbeRunNodeStatus::Running,
            progress: Some(current),
            current: None,
            error: None,
        };
    }

    AdminEndpointProbeRunStatusNode {
        node_id,
        status: AdminEndpointProbeRunNodeStatus::NotFound,
        progress: None,
        current: None,
        error: None,
    }
}

async fn admin_get_endpoint_probe_run_status(
    Extension(state): Extension<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<AdminEndpointProbeRunStatusResponse>, ApiError> {
    if run_id.trim().is_empty() {
        return Err(ApiError::invalid_request("run_id is empty"));
    }

    let nodes = {
        let store = state.store.lock().await;
        store.list_nodes()
    };
    let local_node_id = state.cluster.node_id.clone();

    let Some(ca_key_pem) = state.cluster_ca_key_pem.as_ref().as_deref() else {
        return Err(ApiError::internal("cluster ca key is not available"));
    };

    let client = build_cluster_http_client(&state)?;

    // Note: the admin auth middleware is attached to the `/admin` nested router, so the verifier
    // sees a stripped path like `/_internal/...` (not `/api/admin/...`).
    let uri: axum::http::Uri = format!("/_internal/endpoint-probe/runs/{run_id}")
        .parse()
        .map_err(|_| ApiError::invalid_request("run_id is invalid"))?;
    let sig = internal_auth::sign_request(ca_key_pem, &Method::GET, &uri)
        .map_err(|e| ApiError::internal(format!("sign internal request: {e}")))?;

    let mut tasks = Vec::new();
    for node in nodes {
        let node_id = node.node_id.clone();

        if node_id == local_node_id {
            let handle = state.endpoint_probe.clone();
            let run_id = run_id.clone();
            tasks.push(tokio::spawn(async move {
                let requested = handle
                    .run_snapshot(&run_id)
                    .await
                    .map(map_probe_run_snapshot);
                let current = handle
                    .current_run_snapshot()
                    .await
                    .map(map_probe_run_snapshot);
                map_node_status(&run_id, node_id, requested, current, None)
            }));
            continue;
        }

        let client = client.clone();
        let sig = sig.clone();
        let run_id = run_id.clone();
        let url = format!(
            "{}/api/admin/_internal/endpoint-probe/runs/{}",
            node.api_base_url.trim_end_matches('/'),
            run_id
        );

        tasks.push(tokio::spawn(async move {
            let request = client
                .get(url)
                .header(
                    header::HeaderName::from_static(internal_auth::INTERNAL_SIGNATURE_HEADER),
                    sig,
                )
                .send();

            let resp = tokio::time::timeout(Duration::from_secs(3), request).await;
            let resp = match resp {
                Ok(Ok(resp)) => resp,
                Ok(Err(err)) => {
                    return map_node_status(&run_id, node_id, None, None, Some(err.to_string()));
                }
                Err(_) => {
                    return map_node_status(
                        &run_id,
                        node_id,
                        None,
                        None,
                        Some("timeout".to_string()),
                    );
                }
            };

            if !resp.status().is_success() {
                return map_node_status(
                    &run_id,
                    node_id,
                    None,
                    None,
                    Some(format!("http {}", resp.status())),
                );
            }

            match resp
                .json::<AdminInternalEndpointProbeRunStatusResponse>()
                .await
            {
                Ok(out) => map_node_status(&run_id, node_id, out.requested, out.current, None),
                Err(err) => map_node_status(&run_id, node_id, None, None, Some(err.to_string())),
            }
        }));
    }

    let mut nodes = Vec::new();
    for item in join_all(tasks).await.into_iter().flatten() {
        nodes.push(item);
    }
    nodes.sort_by(|a, b| a.node_id.cmp(&b.node_id));

    let mut hour = None;
    let mut config_hash = None;
    for node in &nodes {
        if let Some(progress) = node.progress.as_ref() {
            hour = Some(progress.hour.clone());
            config_hash = Some(progress.config_hash.clone());
            break;
        }
    }

    let any_running = nodes
        .iter()
        .any(|n| matches!(n.status, AdminEndpointProbeRunNodeStatus::Running));
    let any_failed = nodes
        .iter()
        .any(|n| matches!(n.status, AdminEndpointProbeRunNodeStatus::Failed));
    let any_finished = nodes
        .iter()
        .any(|n| matches!(n.status, AdminEndpointProbeRunNodeStatus::Finished));
    let any_unknown = nodes
        .iter()
        .any(|n| matches!(n.status, AdminEndpointProbeRunNodeStatus::Unknown));

    let status = if any_running {
        AdminEndpointProbeRunOverallStatus::Running
    } else if any_failed {
        AdminEndpointProbeRunOverallStatus::Failed
    } else if any_finished {
        AdminEndpointProbeRunOverallStatus::Finished
    } else if any_unknown {
        AdminEndpointProbeRunOverallStatus::Unknown
    } else {
        AdminEndpointProbeRunOverallStatus::NotFound
    };

    Ok(Json(AdminEndpointProbeRunStatusResponse {
        run_id,
        status,
        hour,
        config_hash,
        nodes,
    }))
}

fn sse_json_event<T: Serialize>(event: &'static str, payload: &T) -> Event {
    match serde_json::to_string(payload) {
        Ok(data) => Event::default().event(event).data(data),
        Err(err) => Event::default().event("error").data(err.to_string()),
    }
}

async fn forward_local_endpoint_probe_run_events(
    handle: crate::endpoint_probe::EndpointProbeHandle,
    node_id: String,
    run_id: String,
    tx: mpsc::Sender<Event>,
) {
    let requested = handle.run_snapshot(&run_id).await;
    let current = handle.current_run_snapshot().await;

    let Some(initial) = requested.or_else(|| {
        current.and_then(|snapshot| {
            if snapshot.run_id == run_id {
                Some(snapshot)
            } else {
                None
            }
        })
    }) else {
        let _ = tx
            .send(sse_json_event(
                "not_found",
                &AdminEndpointProbeRunSseNotFound { node_id, run_id },
            ))
            .await;
        return;
    };

    let progress = map_probe_run_snapshot(initial.clone());
    if tx
        .send(sse_json_event(
            "progress",
            &AdminEndpointProbeRunSseNodeProgress {
                node_id: node_id.clone(),
                progress,
            },
        ))
        .await
        .is_err()
    {
        return;
    }

    if !matches!(
        initial.status,
        crate::endpoint_probe::EndpointProbeRunStatus::Running
    ) {
        return;
    }

    let mut rx = handle.subscribe();
    loop {
        if tx.is_closed() {
            return;
        }

        match rx.recv().await {
            Ok(crate::endpoint_probe::EndpointProbeEvent::RunSnapshot(snapshot)) => {
                if snapshot.run_id != run_id {
                    continue;
                }
                let progress = map_probe_run_snapshot(snapshot.clone());
                if tx
                    .send(sse_json_event(
                        "progress",
                        &AdminEndpointProbeRunSseNodeProgress {
                            node_id: node_id.clone(),
                            progress,
                        },
                    ))
                    .await
                    .is_err()
                {
                    return;
                }

                if !matches!(
                    snapshot.status,
                    crate::endpoint_probe::EndpointProbeRunStatus::Running
                ) {
                    return;
                }
            }
            Ok(crate::endpoint_probe::EndpointProbeEvent::EndpointSample {
                run_id: sample_run_id,
                hour,
                from_node_id,
                sample,
            }) => {
                if sample_run_id != run_id {
                    continue;
                }
                if tx
                    .send(sse_json_event(
                        "sample",
                        &AdminEndpointProbeRunSseEndpointSample {
                            node_id: from_node_id,
                            run_id: sample_run_id,
                            hour,
                            sample,
                        },
                    ))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                let _ = tx
                    .send(sse_json_event(
                        "lagged",
                        &AdminEndpointProbeRunSseLagged {
                            node_id: node_id.clone(),
                            run_id: run_id.clone(),
                            missed,
                        },
                    ))
                    .await;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                return;
            }
        }
    }
}

async fn forward_remote_endpoint_probe_run_events(
    client: reqwest::Client,
    url: String,
    sig: String,
    node_id: String,
    run_id: String,
    tx: mpsc::Sender<Event>,
) {
    let resp = match client
        .get(url)
        .header(
            header::HeaderName::from_static(internal_auth::INTERNAL_SIGNATURE_HEADER),
            sig,
        )
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(err) => {
            let _ = tx
                .send(sse_json_event(
                    "node_error",
                    &AdminEndpointProbeRunSseNodeError {
                        node_id,
                        run_id,
                        error: err.to_string(),
                    },
                ))
                .await;
            return;
        }
    };

    if !resp.status().is_success() {
        let _ = tx
            .send(sse_json_event(
                "node_error",
                &AdminEndpointProbeRunSseNodeError {
                    node_id,
                    run_id,
                    error: format!("http {}", resp.status()),
                },
            ))
            .await;
        return;
    }

    let mut buffer = String::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if tx.is_closed() {
            return;
        }

        let bytes = match chunk {
            Ok(bytes) => bytes,
            Err(err) => {
                let _ = tx
                    .send(sse_json_event(
                        "node_error",
                        &AdminEndpointProbeRunSseNodeError {
                            node_id,
                            run_id,
                            error: err.to_string(),
                        },
                    ))
                    .await;
                return;
            }
        };

        let chunk_text = String::from_utf8_lossy(bytes.as_ref()).replace("\r\n", "\n");
        buffer.push_str(&chunk_text);

        while let Some(split) = buffer.find("\n\n") {
            let frame = buffer[..split].to_string();
            buffer = buffer[split + 2..].to_string();

            let mut event_type: Option<String> = None;
            let mut data_lines: Vec<&str> = Vec::new();
            for line in frame.lines() {
                let line = line.trim_end_matches('\r');
                if line.starts_with(':') {
                    continue;
                }
                if let Some(rest) = line.strip_prefix("event:") {
                    event_type = Some(rest.trim().to_string());
                    continue;
                }
                if let Some(rest) = line.strip_prefix("data:") {
                    data_lines.push(rest.trim_start());
                }
            }

            let Some(event_type) = event_type else {
                continue;
            };
            let data = data_lines.join("\n");
            if data.is_empty() {
                continue;
            }

            if tx
                .send(Event::default().event(event_type).data(data))
                .await
                .is_err()
            {
                return;
            }
        }
    }
}

async fn admin_stream_endpoint_probe_run_events(
    Extension(state): Extension<AppState>,
    Path(run_id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>> + Send>, ApiError> {
    if run_id.trim().is_empty() {
        return Err(ApiError::invalid_request("run_id is empty"));
    }

    let nodes = {
        let store = state.store.lock().await;
        store.list_nodes()
    };
    let node_ids: Vec<String> = nodes.iter().map(|n| n.node_id.clone()).collect();

    let hello = AdminEndpointProbeRunSseHello {
        run_id: run_id.clone(),
        connected_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        nodes: node_ids,
    };

    let mut initial_events = VecDeque::new();
    initial_events.push_back(sse_json_event("hello", &hello));

    let local_node_id = state.cluster.node_id.clone();
    let Some(ca_key_pem) = state.cluster_ca_key_pem.as_ref().as_deref() else {
        return Err(ApiError::internal("cluster ca key is not available"));
    };

    let client = build_cluster_http_client(&state)?;
    let uri: axum::http::Uri = format!("/_internal/endpoint-probe/runs/{run_id}/events")
        .parse()
        .map_err(|_| ApiError::invalid_request("run_id is invalid"))?;
    let sig = internal_auth::sign_request(ca_key_pem, &Method::GET, &uri)
        .map_err(|e| ApiError::internal(format!("sign internal request: {e}")))?;

    let (tx, rx) = mpsc::channel::<Event>(512);

    for node in nodes {
        let node_id = node.node_id.clone();

        if node_id == local_node_id {
            let handle = state.endpoint_probe.clone();
            let tx = tx.clone();
            let run_id = run_id.clone();
            tokio::spawn(async move {
                forward_local_endpoint_probe_run_events(handle, node_id, run_id, tx).await;
            });
            continue;
        }

        let client = client.clone();
        let sig = sig.clone();
        let tx = tx.clone();
        let run_id = run_id.clone();
        let url = format!(
            "{}/api/admin/_internal/endpoint-probe/runs/{}/events",
            node.api_base_url.trim_end_matches('/'),
            run_id
        );

        tokio::spawn(async move {
            forward_remote_endpoint_probe_run_events(client, url, sig, node_id, run_id, tx).await;
        });
    }

    drop(tx);

    let out_stream = stream::unfold((initial_events, rx), |(mut initial, mut rx)| async move {
        if let Some(event) = initial.pop_front() {
            return Some((Ok(event), (initial, rx)));
        }

        let next = rx.recv().await?;
        Some((Ok(next), (initial, rx)))
    });

    Ok(Sse::new(out_stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("keepalive"),
    ))
}

async fn admin_internal_endpoint_probe_run(
    Extension(state): Extension<AppState>,
    ApiJson(req): ApiJson<AdminInternalEndpointProbeRunRequest>,
) -> Result<Json<AdminInternalEndpointProbeRunResponse>, ApiError> {
    if req.run_id.trim().is_empty() {
        return Err(ApiError::invalid_request("run_id is empty"));
    }
    if req.hour.trim().is_empty() {
        return Err(ApiError::invalid_request("hour is empty"));
    }

    let local_hash = crate::endpoint_probe::probe_config_hash();
    if local_hash != req.config_hash {
        return Err(ApiError::conflict(format!(
            "probe config hash mismatch: expected {local_hash}, got {}",
            req.config_hash
        )));
    }

    let accepted = state
        .endpoint_probe
        .start_background(crate::endpoint_probe::EndpointProbeRunRequest {
            hour: req.hour.clone(),
            run_id: req.run_id.clone(),
            config_hash: req.config_hash.clone(),
            reason: "internal",
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(Json(AdminInternalEndpointProbeRunResponse {
        accepted: accepted.accepted,
        already_running: accepted.already_running,
        run_id: req.run_id,
        hour: req.hour,
    }))
}

async fn admin_internal_endpoint_probe_run_status(
    Extension(state): Extension<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<AdminInternalEndpointProbeRunStatusResponse>, ApiError> {
    if run_id.trim().is_empty() {
        return Err(ApiError::invalid_request("run_id is empty"));
    }

    let requested = state
        .endpoint_probe
        .run_snapshot(&run_id)
        .await
        .map(map_probe_run_snapshot);
    let current = state
        .endpoint_probe
        .current_run_snapshot()
        .await
        .map(map_probe_run_snapshot);

    Ok(Json(AdminInternalEndpointProbeRunStatusResponse {
        requested_run_id: run_id,
        requested,
        current,
    }))
}

async fn admin_internal_endpoint_probe_run_events(
    Extension(state): Extension<AppState>,
    Path(run_id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>> + Send>, ApiError> {
    if run_id.trim().is_empty() {
        return Err(ApiError::invalid_request("run_id is empty"));
    }

    let node_id = state.cluster.node_id.clone();
    let handle = state.endpoint_probe.clone();

    let (tx, rx) = mpsc::channel::<Event>(512);
    tokio::spawn(async move {
        forward_local_endpoint_probe_run_events(handle, node_id, run_id, tx).await;
    });

    let out_stream = stream::unfold(rx, |mut rx| async move {
        let next = rx.recv().await?;
        Some((Ok(next), rx))
    });

    Ok(Sse::new(out_stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("keepalive"),
    ))
}

async fn admin_patch_endpoint(
    Extension(state): Extension<AppState>,
    Path(endpoint_id): Path<String>,
    ApiJson(req): ApiJson<PatchEndpointRequest>,
) -> Result<Json<Endpoint>, ApiError> {
    let (mut endpoint, nodes, endpoints) = {
        let store = state.store.lock().await;
        let endpoint = store
            .get_endpoint(&endpoint_id)
            .ok_or_else(|| ApiError::not_found(format!("endpoint not found: {endpoint_id}")))?;
        (endpoint, store.list_nodes(), store.list_endpoints())
    };

    let desired_node_id = match req.node_id {
        None => endpoint.node_id.clone(),
        Some(None) => {
            return Err(ApiError::invalid_request("node_id cannot be null"));
        }
        Some(Some(node_id)) => {
            if node_id.trim().is_empty() {
                return Err(ApiError::invalid_request("node_id is empty"));
            }
            if !nodes.iter().any(|n| n.node_id == node_id) {
                return Err(ApiError::invalid_request(format!(
                    "node not found: {node_id}"
                )));
            }
            node_id
        }
    };

    let desired_port = req.port.unwrap_or(endpoint.port);
    if endpoints.iter().any(|e| {
        e.endpoint_id != endpoint.endpoint_id
            && e.node_id == desired_node_id
            && e.port == desired_port
    }) {
        return Err(ApiError::conflict(format!(
            "endpoint port conflict on target node: node_id={desired_node_id} port={desired_port}"
        )));
    }

    endpoint.node_id = desired_node_id;
    endpoint.port = desired_port;

    match endpoint.kind {
        EndpointKind::VlessRealityVisionTcp => {
            let mut meta: VlessRealityVisionTcpEndpointMeta =
                serde_json::from_value(endpoint.meta.clone())
                    .map_err(|e| ApiError::internal(e.to_string()))?;

            if let Some(reality) = req.reality {
                let Some(reality) = reality else {
                    return Err(ApiError::invalid_request(
                        "reality cannot be null for vless endpoints",
                    ));
                };
                meta.reality = crate::protocol::RealityConfig {
                    dest: reality.dest,
                    server_names: reality.server_names,
                    server_names_source: reality.server_names_source,
                    fingerprint: reality.fingerprint,
                };
            }

            endpoint.meta =
                serde_json::to_value(meta).map_err(|e| ApiError::internal(e.to_string()))?;
        }
        EndpointKind::Ss2022_2022Blake3Aes128Gcm => {
            if req.reality.is_some() {
                return Err(ApiError::invalid_request(
                    "ss2022 endpoints only support port updates",
                ));
            }
        }
    }

    let _ = raft_write(
        &state,
        crate::state::DesiredStateCommand::UpsertEndpoint {
            endpoint: endpoint.clone(),
        },
    )
    .await?;
    state.reconcile.request_full();
    Ok(Json(endpoint))
}

async fn admin_delete_endpoint(
    Extension(state): Extension<AppState>,
    Path(endpoint_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let tag = {
        let store = state.store.lock().await;
        let endpoint = store
            .get_endpoint(&endpoint_id)
            .ok_or_else(|| ApiError::not_found(format!("endpoint not found: {endpoint_id}")))?;
        endpoint.tag
    };

    let out = raft_write(
        &state,
        crate::state::DesiredStateCommand::DeleteEndpoint {
            endpoint_id: endpoint_id.clone(),
        },
    )
    .await?;
    let crate::state::DesiredStateApplyResult::EndpointDeleted { deleted } = out else {
        return Err(ApiError::internal("unexpected raft apply result"));
    };
    if !deleted {
        return Err(ApiError::not_found(format!(
            "endpoint not found: {endpoint_id}"
        )));
    }
    state.reconcile.request_remove_inbound(tag);
    state.reconcile.request_full();
    Ok(StatusCode::NO_CONTENT)
}

async fn admin_list_reality_domains(
    Extension(state): Extension<AppState>,
) -> Result<Json<Items<RealityDomain>>, ApiError> {
    let store = state.store.lock().await;
    Ok(Json(Items {
        items: store.list_reality_domains(),
    }))
}

async fn admin_create_reality_domain(
    Extension(state): Extension<AppState>,
    ApiJson(req): ApiJson<CreateRealityDomainRequest>,
) -> Result<Json<RealityDomain>, ApiError> {
    let domain = RealityDomain {
        domain_id: crate::id::new_ulid_string(),
        server_name: req.server_name,
        disabled_node_ids: req.disabled_node_ids,
    };

    let _ = raft_write(
        &state,
        DesiredStateCommand::CreateRealityDomain {
            domain: domain.clone(),
        },
    )
    .await?;
    state.reconcile.request_full();
    Ok(Json(domain))
}

async fn admin_patch_reality_domain(
    Extension(state): Extension<AppState>,
    Path(domain_id): Path<String>,
    ApiJson(req): ApiJson<PatchRealityDomainRequest>,
) -> Result<Json<RealityDomain>, ApiError> {
    let _ = raft_write(
        &state,
        DesiredStateCommand::PatchRealityDomain {
            domain_id: domain_id.clone(),
            server_name: req.server_name,
            disabled_node_ids: req.disabled_node_ids,
        },
    )
    .await?;
    state.reconcile.request_full();

    let store = state.store.lock().await;
    let domain = store
        .get_reality_domain(&domain_id)
        .ok_or_else(|| ApiError::not_found(format!("reality domain not found: {domain_id}")))?;
    Ok(Json(domain))
}

async fn admin_delete_reality_domain(
    Extension(state): Extension<AppState>,
    Path(domain_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let _ = raft_write(
        &state,
        DesiredStateCommand::DeleteRealityDomain { domain_id },
    )
    .await?;
    state.reconcile.request_full();
    Ok(StatusCode::NO_CONTENT)
}

async fn admin_reorder_reality_domains(
    Extension(state): Extension<AppState>,
    ApiJson(req): ApiJson<ReorderRealityDomainsRequest>,
) -> Result<StatusCode, ApiError> {
    let _ = raft_write(
        &state,
        DesiredStateCommand::ReorderRealityDomains {
            domain_ids: req.domain_ids,
        },
    )
    .await?;
    state.reconcile.request_full();
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct RotateShortIdResponse {
    endpoint_id: String,
    active_short_id: String,
    short_ids: Vec<String>,
}

async fn admin_rotate_short_id(
    Extension(state): Extension<AppState>,
    Path(endpoint_id): Path<String>,
) -> Result<Json<RotateShortIdResponse>, ApiError> {
    let (cmd, out) = {
        let store = state.store.lock().await;

        let endpoint = store
            .get_endpoint(&endpoint_id)
            .ok_or_else(|| ApiError::not_found(format!("endpoint not found: {endpoint_id}")))?;

        if endpoint.kind != EndpointKind::VlessRealityVisionTcp {
            return Err(ApiError::invalid_request(
                "rotate-shortid is only supported for vless_reality_vision_tcp endpoints",
            ));
        }

        let mut rng = rand::rngs::OsRng;
        store
            .build_rotate_vless_reality_short_id_command(&endpoint_id, &mut rng)?
            .ok_or_else(|| ApiError::not_found(format!("endpoint not found: {endpoint_id}")))?
    };

    let _ = raft_write(&state, cmd).await?;
    state.reconcile.request_rebuild_inbound(endpoint_id.clone());

    Ok(Json(RotateShortIdResponse {
        endpoint_id,
        active_short_id: out.active_short_id,
        short_ids: out.short_ids,
    }))
}

async fn admin_create_user(
    Extension(state): Extension<AppState>,
    ApiJson(req): ApiJson<CreateUserRequest>,
) -> Result<Json<User>, ApiError> {
    let user = {
        let store = state.store.lock().await;
        store.build_user(req.display_name, req.quota_reset)?
    };
    let _ = raft_write(
        &state,
        crate::state::DesiredStateCommand::UpsertUser { user: user.clone() },
    )
    .await?;
    Ok(Json(user))
}

async fn admin_list_users(
    Extension(state): Extension<AppState>,
) -> Result<Json<Items<User>>, ApiError> {
    let store = state.store.lock().await;
    Ok(Json(Items {
        items: store.list_users(),
    }))
}

async fn admin_get_user(
    Extension(state): Extension<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<User>, ApiError> {
    let store = state.store.lock().await;
    let user = store
        .get_user(&user_id)
        .ok_or_else(|| ApiError::not_found(format!("user not found: {user_id}")))?;
    Ok(Json(user))
}

async fn admin_patch_user(
    Extension(state): Extension<AppState>,
    Path(user_id): Path<String>,
    ApiJson(req): ApiJson<PatchUserRequest>,
) -> Result<Json<User>, ApiError> {
    let mut user = {
        let store = state.store.lock().await;
        store
            .get_user(&user_id)
            .ok_or_else(|| ApiError::not_found(format!("user not found: {user_id}")))?
    };

    if let Some(display_name) = req.display_name {
        let Some(display_name) = display_name else {
            return Err(ApiError::invalid_request("display_name cannot be null"));
        };
        user.display_name = display_name;
    }
    if let Some(priority_tier) = req.priority_tier {
        user.priority_tier = priority_tier;
    }
    if let Some(quota_reset) = req.quota_reset {
        user.quota_reset = quota_reset;
    }

    let _ = raft_write(
        &state,
        crate::state::DesiredStateCommand::UpsertUser { user: user.clone() },
    )
    .await?;
    Ok(Json(user))
}

async fn admin_delete_user(
    Extension(state): Extension<AppState>,
    Path(user_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let out = raft_write(
        &state,
        crate::state::DesiredStateCommand::DeleteUser {
            user_id: user_id.clone(),
        },
    )
    .await?;
    let crate::state::DesiredStateApplyResult::UserDeleted { deleted } = out else {
        return Err(ApiError::internal("unexpected raft apply result"));
    };
    if !deleted {
        return Err(ApiError::not_found(format!("user not found: {user_id}")));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct ResetTokenResponse {
    subscription_token: String,
}

async fn admin_reset_user_token(
    Extension(state): Extension<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<ResetTokenResponse>, ApiError> {
    let subscription_token = format!("sub_{}", crate::id::new_ulid_string());
    let out = raft_write(
        &state,
        crate::state::DesiredStateCommand::ResetUserSubscriptionToken {
            user_id: user_id.clone(),
            subscription_token: subscription_token.clone(),
        },
    )
    .await?;
    let crate::state::DesiredStateApplyResult::UserTokenReset { applied } = out else {
        return Err(ApiError::internal("unexpected raft apply result"));
    };
    if !applied {
        return Err(ApiError::not_found(format!("user not found: {user_id}")));
    }
    Ok(Json(ResetTokenResponse { subscription_token }))
}

async fn admin_list_user_node_quotas(
    Extension(state): Extension<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<Items<UserNodeQuota>>, ApiError> {
    let store = state.store.lock().await;
    let items = store.list_user_node_quotas(&user_id)?;
    Ok(Json(Items { items }))
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct AdminUserNodeWeightItem {
    node_id: String,
    weight: u16,
}

async fn admin_list_user_node_weights(
    Extension(state): Extension<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<Items<AdminUserNodeWeightItem>>, ApiError> {
    let store = state.store.lock().await;
    let items = store
        .list_user_node_weights(&user_id)?
        .into_iter()
        .map(|(node_id, weight)| AdminUserNodeWeightItem { node_id, weight })
        .collect();
    Ok(Json(Items { items }))
}

#[derive(Debug, Deserialize)]
struct PutUserNodeWeightRequest {
    weight: u16,
}

async fn admin_put_user_node_weight(
    Extension(state): Extension<AppState>,
    Path((user_id, node_id)): Path<(String, String)>,
    ApiJson(req): ApiJson<PutUserNodeWeightRequest>,
) -> Result<Json<AdminUserNodeWeightItem>, ApiError> {
    let _ = raft_write(
        &state,
        DesiredStateCommand::SetUserNodeWeight {
            user_id: user_id.clone(),
            node_id: node_id.clone(),
            weight: req.weight,
        },
    )
    .await?;
    Ok(Json(AdminUserNodeWeightItem {
        node_id,
        weight: req.weight,
    }))
}

#[derive(Debug, Deserialize)]
struct ScopeQuery {
    scope: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuotaResetPolicy {
    Monthly,
    Unlimited,
}

fn resolve_user_node_quota_reset_for_status(
    store: &JsonSnapshotStore,
    user_id: &str,
    node_id: &str,
) -> Result<(QuotaResetSource, QuotaResetPolicy, CycleTimeZone, u8), ApiError> {
    let source = store
        .get_user_node_quota_reset_source(user_id, node_id)
        .unwrap_or_default();

    let (policy, day_of_month, tz) = match source {
        QuotaResetSource::User => {
            let user = store
                .get_user(user_id)
                .ok_or_else(|| ApiError::not_found(format!("user not found: {user_id}")))?;
            match user.quota_reset {
                UserQuotaReset::Unlimited { tz_offset_minutes } => (
                    QuotaResetPolicy::Unlimited,
                    1,
                    CycleTimeZone::FixedOffsetMinutes { tz_offset_minutes },
                ),
                UserQuotaReset::Monthly {
                    day_of_month,
                    tz_offset_minutes,
                } => (
                    QuotaResetPolicy::Monthly,
                    day_of_month,
                    CycleTimeZone::FixedOffsetMinutes { tz_offset_minutes },
                ),
            }
        }
        QuotaResetSource::Node => {
            let node = store
                .get_node(node_id)
                .ok_or_else(|| ApiError::not_found(format!("node not found: {node_id}")))?;
            match node.quota_reset {
                NodeQuotaReset::Unlimited { tz_offset_minutes } => (
                    QuotaResetPolicy::Unlimited,
                    1,
                    match tz_offset_minutes {
                        Some(tz_offset_minutes) => {
                            CycleTimeZone::FixedOffsetMinutes { tz_offset_minutes }
                        }
                        None => CycleTimeZone::Local,
                    },
                ),
                NodeQuotaReset::Monthly {
                    day_of_month,
                    tz_offset_minutes,
                } => (
                    QuotaResetPolicy::Monthly,
                    day_of_month,
                    match tz_offset_minutes {
                        Some(tz_offset_minutes) => {
                            CycleTimeZone::FixedOffsetMinutes { tz_offset_minutes }
                        }
                        None => CycleTimeZone::Local,
                    },
                ),
            }
        }
    };

    if !(1..=31).contains(&day_of_month) {
        return Err(ApiError::internal(format!(
            "invalid day_of_month: {day_of_month}"
        )));
    }

    Ok((source, policy, tz, day_of_month))
}

fn resolve_node_quota_reset_for_status(
    store: &JsonSnapshotStore,
    node_id: &str,
) -> Result<(QuotaResetPolicy, CycleTimeZone, u8), ApiError> {
    let node = store
        .get_node(node_id)
        .ok_or_else(|| ApiError::not_found(format!("node not found: {node_id}")))?;

    let (policy, day_of_month, tz) = match node.quota_reset {
        NodeQuotaReset::Unlimited { tz_offset_minutes } => (
            QuotaResetPolicy::Unlimited,
            1,
            match tz_offset_minutes {
                Some(tz_offset_minutes) => CycleTimeZone::FixedOffsetMinutes { tz_offset_minutes },
                None => CycleTimeZone::Local,
            },
        ),
        NodeQuotaReset::Monthly {
            day_of_month,
            tz_offset_minutes,
        } => (
            QuotaResetPolicy::Monthly,
            day_of_month,
            match tz_offset_minutes {
                Some(tz_offset_minutes) => CycleTimeZone::FixedOffsetMinutes { tz_offset_minutes },
                None => CycleTimeZone::Local,
            },
        ),
    };

    if !(1..=31).contains(&day_of_month) {
        return Err(ApiError::internal(format!(
            "invalid day_of_month: {day_of_month}"
        )));
    }

    Ok((policy, tz, day_of_month))
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AdminUserQuotaLimitKind {
    /// A true "unlimited" quota (no budget) for the relevant scope.
    Unlimited,
    /// A static, per-user quota limit (legacy mode).
    Fixed,
    /// Shared node quota: this is the user's derived base share of the node budget.
    SharedBase,
    /// Shared node quota: no base share; user can only consume overflow.
    SharedOpportunistic,
    /// Aggregated across nodes where quota kinds differ (non-unlimited).
    Mixed,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct AdminUserQuotaSummaryItem {
    user_id: String,
    quota_limit_kind: AdminUserQuotaLimitKind,
    quota_limit_bytes: u64,
    used_bytes: u64,
    remaining_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct AdminUserQuotaSummariesResponse {
    partial: bool,
    unreachable_nodes: Vec<String>,
    items: Vec<AdminUserQuotaSummaryItem>,
}

#[derive(Debug, Deserialize)]
struct AdminUserQuotaSummaryItemWire {
    user_id: String,
    #[serde(default)]
    quota_limit_kind: Option<AdminUserQuotaLimitKind>,
    quota_limit_bytes: u64,
    used_bytes: u64,
    remaining_bytes: u64,
}

#[derive(Debug, Deserialize)]
struct AdminUserQuotaSummariesResponseWire {
    items: Vec<AdminUserQuotaSummaryItemWire>,
}

fn normalize_quota_limit_kind(
    quota_limit_bytes: u64,
    kind: Option<AdminUserQuotaLimitKind>,
) -> AdminUserQuotaLimitKind {
    // Backward-compat: older nodes didn't return `quota_limit_kind`; they used
    // `quota_limit_bytes == 0` to mean "unlimited".
    kind.unwrap_or({
        if quota_limit_bytes == 0 {
            AdminUserQuotaLimitKind::Unlimited
        } else {
            AdminUserQuotaLimitKind::Fixed
        }
    })
}

fn merge_quota_limit_kind(
    a: &AdminUserQuotaLimitKind,
    b: &AdminUserQuotaLimitKind,
) -> AdminUserQuotaLimitKind {
    use AdminUserQuotaLimitKind as K;
    match (a, b) {
        (K::Unlimited, _) | (_, K::Unlimited) => K::Unlimited,
        (x, y) if x == y => x.clone(),
        _ => K::Mixed,
    }
}

fn build_local_user_quota_summaries(
    store: &JsonSnapshotStore,
    local_node_id: &str,
) -> Result<Vec<AdminUserQuotaSummaryItem>, ApiError> {
    let now = Utc::now();

    let local_node_quota_limit_bytes = store
        .get_node(local_node_id)
        .map(|n| n.quota_limit_bytes)
        .unwrap_or(0);
    let shared_quota_enabled = local_node_quota_limit_bytes > 0;

    let endpoints_by_id = store
        .list_endpoints()
        .into_iter()
        .map(|e| (e.endpoint_id.clone(), e))
        .collect::<std::collections::BTreeMap<_, _>>();

    // Group grants by (user_id, node_id=local) to match quota enforcement behavior.
    let mut grants_by_user: std::collections::BTreeMap<String, Vec<Grant>> =
        std::collections::BTreeMap::new();
    for grant in store.list_grants() {
        // Keep behavior consistent with quota enforcement: if an endpoint is missing (e.g. deleted
        // while grants still exist), treat the grant as belonging to the local node.
        if let Some(endpoint) = endpoints_by_id.get(&grant.endpoint_id)
            && endpoint.node_id != local_node_id
        {
            continue;
        }
        grants_by_user
            .entry(grant.user_id.clone())
            .or_default()
            .push(grant);
    }

    // In shared-quota mode, the per-user "limit" is derived from the node budget and the user's
    // tier/weight. Compute it once so the loop can stay simple and stable.
    let mut shared_cycle: Option<(QuotaResetPolicy, CycleTimeZone, u8)> = None;
    let mut shared_base_by_user: std::collections::BTreeMap<String, u64> =
        std::collections::BTreeMap::new();
    if shared_quota_enabled {
        let (policy, tz, day_of_month) = resolve_node_quota_reset_for_status(store, local_node_id)?;
        shared_cycle = Some((policy, tz, day_of_month));

        if policy != QuotaResetPolicy::Unlimited {
            // Only allocate base quota among enabled P1/P2 users (matching enforcement behavior).
            let mut items: Vec<(String, u16)> = Vec::new();
            for (user_id, grants) in grants_by_user.iter() {
                if user_id == crate::endpoint_probe::PROBE_USER_ID {
                    continue;
                }
                if !grants.iter().any(|g| g.enabled) {
                    continue;
                }
                let tier = store
                    .get_user(user_id)
                    .map(|u| u.priority_tier)
                    .unwrap_or_default();
                if tier == crate::domain::UserPriorityTier::P3 {
                    continue;
                }
                let weight = store.resolve_user_node_weight(user_id, local_node_id);
                items.push((user_id.clone(), weight));
            }
            items.sort_by(|a, b| a.0.cmp(&b.0));
            items.dedup_by(|a, b| a.0 == b.0);

            let distributable =
                crate::quota_policy::distributable_bytes(local_node_quota_limit_bytes);
            for (user_id, base) in
                crate::quota_policy::allocate_total_by_weight(distributable, &items)
            {
                shared_base_by_user.insert(user_id, base);
            }
        }
    }

    let mut items = Vec::new();
    for user in store.list_users() {
        let user_id = user.user_id;
        let grants = grants_by_user.remove(&user_id).unwrap_or_default();

        // Under the shared node quota policy, per-user quota summaries are derived from the
        // node's quota budget + quota_reset (not from per-grant/static quotas).
        let (quota_limit_kind, quota_limit_bytes, policy, tz, day_of_month) =
            if shared_quota_enabled {
                if grants.is_empty() {
                    continue;
                }
                let (policy, tz, day_of_month) = shared_cycle
                    .as_ref()
                    .cloned()
                    .expect("shared_cycle is set when shared_quota_enabled");

                if policy == QuotaResetPolicy::Unlimited {
                    (
                        AdminUserQuotaLimitKind::Unlimited,
                        0,
                        policy,
                        tz,
                        day_of_month,
                    )
                } else {
                    let tier = store
                        .get_user(&user_id)
                        .map(|u| u.priority_tier)
                        .unwrap_or_default();
                    if tier == crate::domain::UserPriorityTier::P3 {
                        // P3 has no fixed base share; it can only consume overflow.
                        (
                            AdminUserQuotaLimitKind::SharedOpportunistic,
                            0,
                            policy,
                            tz,
                            day_of_month,
                        )
                    } else {
                        let base = shared_base_by_user.get(&user_id).copied().unwrap_or(0);
                        (
                            AdminUserQuotaLimitKind::SharedBase,
                            base,
                            policy,
                            tz,
                            day_of_month,
                        )
                    }
                }
            } else {
                let explicit = store.get_user_node_quota_limit_bytes(&user_id, local_node_id);
                let uniform_grant_quota = {
                    let first = grants.first().map(|g| g.quota_limit_bytes);
                    if let Some(first) = first
                        && grants.iter().all(|g| g.quota_limit_bytes == first)
                    {
                        Some(first)
                    } else {
                        None
                    }
                };
                let Some(quota_limit_bytes) = explicit.or(uniform_grant_quota) else {
                    continue;
                };

                let (_source, policy, tz, day_of_month) =
                    resolve_user_node_quota_reset_for_status(store, &user_id, local_node_id)?;

                let quota_limit_kind =
                    if policy == QuotaResetPolicy::Unlimited || quota_limit_bytes == 0 {
                        AdminUserQuotaLimitKind::Unlimited
                    } else {
                        AdminUserQuotaLimitKind::Fixed
                    };
                let quota_limit_bytes = if quota_limit_kind == AdminUserQuotaLimitKind::Unlimited {
                    0
                } else {
                    quota_limit_bytes
                };
                (
                    quota_limit_kind,
                    quota_limit_bytes,
                    policy,
                    tz,
                    day_of_month,
                )
            };

        let (cycle_start_at, cycle_end_at) = if policy == QuotaResetPolicy::Monthly {
            let (cycle_start, cycle_end) = current_cycle_window_at(tz, day_of_month, now)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            (Some(cycle_start.to_rfc3339()), Some(cycle_end.to_rfc3339()))
        } else {
            (None, None)
        };

        let used_bytes = grants.iter().fold(0u64, |acc, g| {
            let usage = store.get_grant_usage(&g.grant_id);
            let Some(usage) = usage else {
                return acc;
            };
            if let (Some(expected_start), Some(expected_end)) = (&cycle_start_at, &cycle_end_at)
                && (usage.cycle_start_at != *expected_start || usage.cycle_end_at != *expected_end)
            {
                return acc;
            }
            acc.saturating_add(usage.used_bytes)
        });

        let remaining_bytes = match quota_limit_kind {
            AdminUserQuotaLimitKind::Unlimited | AdminUserQuotaLimitKind::SharedOpportunistic => 0,
            _ => quota_limit_bytes.saturating_sub(used_bytes),
        };
        items.push(AdminUserQuotaSummaryItem {
            user_id,
            quota_limit_kind,
            quota_limit_bytes,
            used_bytes,
            remaining_bytes,
        });
    }

    Ok(items)
}

async fn admin_list_user_quota_summaries(
    Extension(state): Extension<AppState>,
    Query(query): Query<ScopeQuery>,
) -> Result<Json<AdminUserQuotaSummariesResponse>, ApiError> {
    if let Some(scope) = query.scope.as_deref()
        && scope != "local"
    {
        return Err(ApiError::invalid_request(
            "invalid scope, expected local or omit",
        ));
    }

    let local_node_id = state.cluster.node_id.clone();
    let local_items = {
        let store = state.store.lock().await;
        build_local_user_quota_summaries(&store, &local_node_id)?
    };

    if query.scope.as_deref() == Some("local") {
        return Ok(Json(AdminUserQuotaSummariesResponse {
            partial: false,
            unreachable_nodes: Vec::new(),
            items: local_items,
        }));
    }

    let nodes = {
        let store = state.store.lock().await;
        store.list_nodes()
    };
    let client = build_admin_http_client(state.cluster_ca_pem.as_str())?;
    let ca_key_pem = state
        .cluster_ca_key_pem
        .as_ref()
        .as_ref()
        .ok_or_else(|| ApiError::internal("cluster ca key is not available on this node"))?;

    // Note: the admin auth middleware is attached to the `/admin` nested router, so the
    // verifier sees a stripped path like `/users/quota-summaries?...` (not `/api/admin/...`).
    let local_uri: axum::http::Uri = "/users/quota-summaries?scope=local"
        .parse()
        .expect("valid uri");
    let sig = internal_auth::sign_request(ca_key_pem, &Method::GET, &local_uri)
        .map_err(ApiError::internal)?;

    let mut unreachable_nodes = Vec::new();

    let mut totals: std::collections::BTreeMap<String, AdminUserQuotaSummaryItem> =
        std::collections::BTreeMap::new();

    for item in local_items {
        totals.insert(item.user_id.clone(), item);
    }

    for node in nodes {
        if node.node_id == local_node_id {
            continue;
        }
        let base = node.api_base_url.trim_end_matches('/');
        if base.is_empty() {
            unreachable_nodes.push(node.node_id);
            continue;
        }
        let url = format!("{base}/api/admin/users/quota-summaries?scope=local");
        let request = client
            .get(url)
            .header(
                header::HeaderName::from_static(internal_auth::INTERNAL_SIGNATURE_HEADER),
                sig.clone(),
            )
            .send();
        let response = tokio::time::timeout(Duration::from_secs(3), request).await;
        let response = match response {
            Ok(Ok(response)) => response,
            _ => {
                unreachable_nodes.push(node.node_id);
                continue;
            }
        };

        if !response.status().is_success() {
            unreachable_nodes.push(node.node_id);
            continue;
        }

        match response.json::<AdminUserQuotaSummariesResponseWire>().await {
            Ok(remote) => {
                for wire in remote.items {
                    let item = AdminUserQuotaSummaryItem {
                        user_id: wire.user_id,
                        quota_limit_kind: normalize_quota_limit_kind(
                            wire.quota_limit_bytes,
                            wire.quota_limit_kind,
                        ),
                        quota_limit_bytes: wire.quota_limit_bytes,
                        used_bytes: wire.used_bytes,
                        remaining_bytes: wire.remaining_bytes,
                    };
                    totals
                        .entry(item.user_id.clone())
                        .and_modify(|entry| {
                            // Keep semantics consistent with enforcement: any truly unlimited node
                            // makes the aggregated quota unlimited.
                            entry.used_bytes = entry.used_bytes.saturating_add(item.used_bytes);
                            entry.quota_limit_kind = merge_quota_limit_kind(
                                &entry.quota_limit_kind,
                                &item.quota_limit_kind,
                            );

                            if matches!(entry.quota_limit_kind, AdminUserQuotaLimitKind::Unlimited)
                            {
                                entry.quota_limit_bytes = 0;
                                entry.remaining_bytes = 0;
                            } else {
                                entry.quota_limit_bytes = entry
                                    .quota_limit_bytes
                                    .saturating_add(item.quota_limit_bytes);
                                entry.remaining_bytes =
                                    entry.remaining_bytes.saturating_add(item.remaining_bytes);
                            }
                        })
                        .or_insert(item);
                }
            }
            Err(_) => unreachable_nodes.push(node.node_id),
        }
    }

    let partial = !unreachable_nodes.is_empty();
    Ok(Json(AdminUserQuotaSummariesResponse {
        partial,
        unreachable_nodes,
        items: totals.into_values().collect(),
    }))
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct AdminUserNodeQuotaStatusItem {
    user_id: String,
    node_id: String,
    quota_limit_bytes: u64,
    used_bytes: u64,
    remaining_bytes: u64,
    cycle_end_at: Option<String>,
    quota_reset_source: QuotaResetSource,
}

#[derive(Debug, Serialize, Deserialize)]
struct AdminUserNodeQuotaStatusResponse {
    partial: bool,
    unreachable_nodes: Vec<String>,
    items: Vec<AdminUserNodeQuotaStatusItem>,
}

fn build_local_user_node_quota_status(
    store: &JsonSnapshotStore,
    local_node_id: &str,
    user_id: &str,
) -> Result<Vec<AdminUserNodeQuotaStatusItem>, ApiError> {
    let now = Utc::now();
    let endpoints_by_id = store
        .list_endpoints()
        .into_iter()
        .map(|e| (e.endpoint_id.clone(), e))
        .collect::<std::collections::BTreeMap<_, _>>();

    let mut grants = Vec::new();
    for grant in store.list_grants() {
        if grant.user_id != user_id {
            continue;
        }
        // Keep behavior consistent with quota enforcement: if an endpoint is missing (e.g. deleted
        // while grants still exist), treat the grant as belonging to the local node.
        if let Some(endpoint) = endpoints_by_id.get(&grant.endpoint_id)
            && endpoint.node_id != local_node_id
        {
            continue;
        }
        grants.push(grant);
    }

    let explicit = store.get_user_node_quota_limit_bytes(user_id, local_node_id);
    let uniform_grant_quota = {
        let first = grants.first().map(|g| g.quota_limit_bytes);
        if let Some(first) = first
            && grants.iter().all(|g| g.quota_limit_bytes == first)
        {
            Some(first)
        } else {
            None
        }
    };
    let Some(quota_limit_bytes) = explicit.or(uniform_grant_quota) else {
        return Ok(Vec::new());
    };

    let (quota_reset_source, policy, tz, day_of_month) =
        resolve_user_node_quota_reset_for_status(store, user_id, local_node_id)?;
    let cycle_end_at = if policy == QuotaResetPolicy::Monthly {
        let (_cycle_start, cycle_end) = current_cycle_window_at(tz, day_of_month, now)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        Some(cycle_end.to_rfc3339())
    } else {
        None
    };

    let used_bytes = grants.iter().fold(0u64, |acc, g| {
        let usage = store.get_grant_usage(&g.grant_id);
        let Some(usage) = usage else {
            return acc;
        };
        if let Some(expected_end) = &cycle_end_at
            && usage.cycle_end_at != *expected_end
        {
            return acc;
        }
        acc.saturating_add(usage.used_bytes)
    });

    let remaining_bytes = quota_limit_bytes.saturating_sub(used_bytes);
    Ok(vec![AdminUserNodeQuotaStatusItem {
        user_id: user_id.to_string(),
        node_id: local_node_id.to_string(),
        quota_limit_bytes,
        used_bytes,
        remaining_bytes,
        cycle_end_at,
        quota_reset_source,
    }])
}

async fn admin_get_user_node_quota_status(
    Extension(state): Extension<AppState>,
    Path(user_id): Path<String>,
    Query(query): Query<ScopeQuery>,
) -> Result<Json<AdminUserNodeQuotaStatusResponse>, ApiError> {
    if let Some(scope) = query.scope.as_deref()
        && scope != "local"
    {
        return Err(ApiError::invalid_request(
            "invalid scope, expected local or omit",
        ));
    }

    let local_node_id = state.cluster.node_id.clone();
    let local_items = {
        let store = state.store.lock().await;
        if store.get_user(&user_id).is_none() {
            return Err(ApiError::not_found(format!("user not found: {user_id}")));
        }
        build_local_user_node_quota_status(&store, &local_node_id, &user_id)?
    };

    if query.scope.as_deref() == Some("local") {
        return Ok(Json(AdminUserNodeQuotaStatusResponse {
            partial: false,
            unreachable_nodes: Vec::new(),
            items: local_items,
        }));
    }

    let nodes = {
        let store = state.store.lock().await;
        store.list_nodes()
    };
    let client = build_admin_http_client(state.cluster_ca_pem.as_str())?;
    let ca_key_pem = state
        .cluster_ca_key_pem
        .as_ref()
        .as_ref()
        .ok_or_else(|| ApiError::internal("cluster ca key is not available on this node"))?;

    // Note: the admin auth middleware is attached to the `/admin` nested router, so the
    // verifier sees a stripped path like `/users/:user_id/node-quotas/status?...`.
    let local_uri: axum::http::Uri = format!("/users/{user_id}/node-quotas/status?scope=local")
        .parse()
        .expect("valid uri");
    let sig = internal_auth::sign_request(ca_key_pem, &Method::GET, &local_uri)
        .map_err(ApiError::internal)?;

    let mut items = local_items;
    let mut unreachable_nodes = Vec::new();

    for node in nodes {
        if node.node_id == local_node_id {
            continue;
        }
        let base = node.api_base_url.trim_end_matches('/');
        if base.is_empty() {
            unreachable_nodes.push(node.node_id);
            continue;
        }
        let url = format!("{base}/api/admin/users/{user_id}/node-quotas/status?scope=local");
        let request = client
            .get(url)
            .header(
                header::HeaderName::from_static(internal_auth::INTERNAL_SIGNATURE_HEADER),
                sig.clone(),
            )
            .send();
        let response = tokio::time::timeout(Duration::from_secs(3), request).await;
        let response = match response {
            Ok(Ok(response)) => response,
            _ => {
                unreachable_nodes.push(node.node_id);
                continue;
            }
        };

        if !response.status().is_success() {
            unreachable_nodes.push(node.node_id);
            continue;
        }

        match response.json::<AdminUserNodeQuotaStatusResponse>().await {
            Ok(remote) => items.extend(remote.items),
            Err(_) => unreachable_nodes.push(node.node_id),
        }
    }

    let partial = !unreachable_nodes.is_empty();
    Ok(Json(AdminUserNodeQuotaStatusResponse {
        partial,
        unreachable_nodes,
        items,
    }))
}

async fn admin_put_user_node_quota(
    Extension(state): Extension<AppState>,
    Path((user_id, node_id)): Path<(String, String)>,
    ApiJson(req): ApiJson<serde_json::Value>,
) -> Result<Json<UserNodeQuota>, ApiError> {
    // Deprecated: static per-user node quotas can bypass the shared quota policy.
    // Keep the read-only status endpoints; deny writes.
    let _ = (&state, &user_id, &node_id, &req);
    Err(ApiError::new(
        "gone",
        StatusCode::GONE,
        "user node quotas are no longer editable; configure node quota_limit_bytes + user node weights instead",
    ))
}

async fn admin_list_grant_groups(
    Extension(state): Extension<AppState>,
) -> Result<Json<Items<AdminGrantGroupSummary>>, ApiError> {
    let store = state.store.lock().await;
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for grant in store.list_grants() {
        *counts.entry(grant.group_name).or_default() += 1;
    }
    let items = counts
        .into_iter()
        .map(|(group_name, member_count)| AdminGrantGroupSummary {
            group_name,
            member_count,
        })
        .collect();
    Ok(Json(Items { items }))
}

async fn admin_create_grant_group(
    Extension(state): Extension<AppState>,
    ApiJson(req): ApiJson<CreateGrantGroupRequest>,
) -> Result<Json<AdminGrantGroupDetail>, ApiError> {
    let CreateGrantGroupRequest {
        group_name,
        members,
    } = req;
    validate_group_name(&group_name).map_err(|e| ApiError::invalid_request(e.to_string()))?;
    if members.is_empty() {
        return Err(ApiError::invalid_request(
            "grant group must have at least 1 member",
        ));
    }

    let mut grants = Vec::with_capacity(members.len());
    {
        let store = state.store.lock().await;
        for m in members {
            let grant = store.build_grant(
                group_name.clone(),
                m.user_id,
                m.endpoint_id,
                m.quota_limit_bytes,
                m.enabled,
                m.note,
            )?;
            grants.push(grant);
        }
    }

    let out = raft_write(
        &state,
        DesiredStateCommand::CreateGrantGroup {
            group_name: group_name.clone(),
            grants: grants.clone(),
        },
    )
    .await?;
    let crate::state::DesiredStateApplyResult::GrantGroupCreated { .. } = out else {
        return Err(ApiError::internal("unexpected raft apply result"));
    };

    state.reconcile.request_full();

    let mut members: Vec<AdminGrantGroupMember> = grants
        .into_iter()
        .map(|g| AdminGrantGroupMember {
            user_id: g.user_id,
            endpoint_id: g.endpoint_id,
            enabled: g.enabled,
            quota_limit_bytes: g.quota_limit_bytes,
            note: g.note,
            credentials: g.credentials,
        })
        .collect();
    members.sort_by(|a, b| {
        a.user_id
            .cmp(&b.user_id)
            .then_with(|| a.endpoint_id.cmp(&b.endpoint_id))
    });

    Ok(Json(AdminGrantGroupDetail {
        group: AdminGrantGroup { group_name },
        members,
    }))
}

async fn admin_get_grant_group(
    Extension(state): Extension<AppState>,
    Path(group_name): Path<String>,
) -> Result<Json<AdminGrantGroupDetail>, ApiError> {
    validate_group_name(&group_name).map_err(|e| ApiError::invalid_request(e.to_string()))?;
    let store = state.store.lock().await;
    let mut members: Vec<AdminGrantGroupMember> = store
        .list_grants()
        .into_iter()
        .filter(|g| g.group_name == group_name)
        .map(|g| AdminGrantGroupMember {
            user_id: g.user_id,
            endpoint_id: g.endpoint_id,
            enabled: g.enabled,
            quota_limit_bytes: g.quota_limit_bytes,
            note: g.note,
            credentials: g.credentials,
        })
        .collect();

    if members.is_empty() {
        return Err(ApiError::not_found(format!(
            "grant group not found: {group_name}"
        )));
    }

    members.sort_by(|a, b| {
        a.user_id
            .cmp(&b.user_id)
            .then_with(|| a.endpoint_id.cmp(&b.endpoint_id))
    });

    Ok(Json(AdminGrantGroupDetail {
        group: AdminGrantGroup { group_name },
        members,
    }))
}

#[derive(Serialize)]
struct AdminGrantGroupReplaceResponse {
    group: AdminGrantGroup,
    created: usize,
    updated: usize,
    deleted: usize,
}

async fn admin_replace_grant_group(
    Extension(state): Extension<AppState>,
    Path(group_name): Path<String>,
    ApiJson(req): ApiJson<ReplaceGrantGroupRequest>,
) -> Result<Json<AdminGrantGroupReplaceResponse>, ApiError> {
    let ReplaceGrantGroupRequest { rename_to, members } = req;
    validate_group_name(&group_name).map_err(|e| ApiError::invalid_request(e.to_string()))?;
    if let Some(rename_to) = rename_to.as_deref() {
        validate_group_name(rename_to).map_err(|e| ApiError::invalid_request(e.to_string()))?;
    }
    if members.is_empty() {
        return Err(ApiError::invalid_request(
            "grant group must have at least 1 member",
        ));
    }

    let grants = {
        let store = state.store.lock().await;
        let existing: Vec<Grant> = store
            .list_grants()
            .into_iter()
            .filter(|g| g.group_name == group_name)
            .collect();
        if existing.is_empty() {
            return Err(ApiError::not_found(format!(
                "grant group not found: {group_name}"
            )));
        }

        let mut existing_by_pair = std::collections::BTreeMap::<(String, String), Grant>::new();
        for g in existing {
            existing_by_pair.insert((g.user_id.clone(), g.endpoint_id.clone()), g);
        }

        let mut out = Vec::with_capacity(members.len());
        for m in members {
            let key = (m.user_id.clone(), m.endpoint_id.clone());
            if let Some(existing) = existing_by_pair.get(&key) {
                out.push(Grant {
                    grant_id: existing.grant_id.clone(),
                    group_name: group_name.clone(),
                    user_id: m.user_id,
                    endpoint_id: m.endpoint_id,
                    enabled: m.enabled,
                    quota_limit_bytes: m.quota_limit_bytes,
                    note: m.note,
                    credentials: existing.credentials.clone(),
                });
            } else {
                out.push(store.build_grant(
                    group_name.clone(),
                    m.user_id,
                    m.endpoint_id,
                    m.quota_limit_bytes,
                    m.enabled,
                    m.note,
                )?);
            }
        }
        out
    };

    let out = raft_write(
        &state,
        DesiredStateCommand::ReplaceGrantGroup {
            group_name: group_name.clone(),
            rename_to: rename_to.map(Some),
            grants,
        },
    )
    .await?;
    let crate::state::DesiredStateApplyResult::GrantGroupReplaced {
        group_name,
        created,
        updated,
        deleted,
    } = out
    else {
        return Err(ApiError::internal("unexpected raft apply result"));
    };

    state.reconcile.request_full();
    Ok(Json(AdminGrantGroupReplaceResponse {
        group: AdminGrantGroup { group_name },
        created,
        updated,
        deleted,
    }))
}

#[derive(Serialize)]
struct AdminGrantGroupDeleteResponse {
    deleted: usize,
}

async fn admin_delete_grant_group(
    Extension(state): Extension<AppState>,
    Path(group_name): Path<String>,
) -> Result<Json<AdminGrantGroupDeleteResponse>, ApiError> {
    validate_group_name(&group_name).map_err(|e| ApiError::invalid_request(e.to_string()))?;
    let out = raft_write(
        &state,
        DesiredStateCommand::DeleteGrantGroup {
            group_name: group_name.clone(),
        },
    )
    .await?;
    let crate::state::DesiredStateApplyResult::GrantGroupDeleted { deleted } = out else {
        return Err(ApiError::internal("unexpected raft apply result"));
    };
    state.reconcile.request_full();
    Ok(Json(AdminGrantGroupDeleteResponse { deleted }))
}

#[derive(Debug, Deserialize)]
struct AlertsQuery {
    scope: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct AlertItem {
    #[serde(rename = "type")]
    alert_type: String,
    grant_id: String,
    endpoint_id: String,
    owner_node_id: String,
    desired_enabled: bool,
    quota_banned: bool,
    quota_banned_at: Option<String>,
    effective_enabled: bool,
    message: String,
    action_hint: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct AlertsResponse {
    partial: bool,
    unreachable_nodes: Vec<String>,
    items: Vec<AlertItem>,
}

const ALERT_TYPE_QUOTA_ENFORCED: &str = "quota_enforced_but_desired_enabled";
const ALERT_MESSAGE_QUOTA_ENFORCED: &str =
    "quota enforced on owner node but desired state is still enabled";
const ALERT_ACTION_HINT_QUOTA_ENFORCED: &str = "check raft leader/quorum and retry status";

fn build_local_alerts(store: &JsonSnapshotStore, local_node_id: &str) -> Vec<AlertItem> {
    let mut items = Vec::new();
    for grant in store.list_grants() {
        let endpoint = match store.get_endpoint(&grant.endpoint_id) {
            Some(endpoint) => endpoint,
            None => continue,
        };
        if endpoint.node_id != local_node_id {
            continue;
        }
        let usage = match store.get_grant_usage(&grant.grant_id) {
            Some(usage) => usage,
            None => continue,
        };
        if !grant.enabled || !usage.quota_banned {
            continue;
        }
        let effective_enabled = grant.enabled && !usage.quota_banned;
        items.push(AlertItem {
            alert_type: ALERT_TYPE_QUOTA_ENFORCED.to_string(),
            grant_id: grant.grant_id.clone(),
            endpoint_id: endpoint.endpoint_id,
            owner_node_id: endpoint.node_id,
            desired_enabled: grant.enabled,
            quota_banned: usage.quota_banned,
            quota_banned_at: usage.quota_banned_at,
            effective_enabled,
            message: ALERT_MESSAGE_QUOTA_ENFORCED.to_string(),
            action_hint: ALERT_ACTION_HINT_QUOTA_ENFORCED.to_string(),
        });
    }
    items
}

fn build_admin_http_client(cluster_ca_pem: &str) -> Result<reqwest::Client, ApiError> {
    let ca = reqwest::Certificate::from_pem(cluster_ca_pem.as_bytes())
        .map_err(|e| ApiError::internal(e.to_string()))?;
    reqwest::Client::builder()
        .add_root_certificate(ca)
        .build()
        .map_err(|e| ApiError::internal(e.to_string()))
}

async fn admin_get_alerts(
    Extension(state): Extension<AppState>,
    Query(query): Query<AlertsQuery>,
) -> Result<Json<AlertsResponse>, ApiError> {
    if let Some(scope) = query.scope.as_deref()
        && scope != "local"
    {
        return Err(ApiError::invalid_request(
            "invalid scope, expected local or omit",
        ));
    }

    let local_node_id = state.cluster.node_id.clone();
    let local_items = {
        let store = state.store.lock().await;
        build_local_alerts(&store, &local_node_id)
    };

    if query.scope.as_deref() == Some("local") {
        return Ok(Json(AlertsResponse {
            partial: false,
            unreachable_nodes: Vec::new(),
            items: local_items,
        }));
    }

    let nodes = {
        let store = state.store.lock().await;
        store.list_nodes()
    };
    let client = build_admin_http_client(state.cluster_ca_pem.as_str())?;
    let ca_key_pem = state
        .cluster_ca_key_pem
        .as_ref()
        .as_ref()
        .ok_or_else(|| ApiError::internal("cluster ca key is not available on this node"))?;

    let mut items = local_items;
    let mut unreachable_nodes = Vec::new();

    // Note: the admin auth middleware is attached to the `/admin` nested router, so the
    // verifier sees a stripped path like `/alerts?...` (not `/api/admin/...`).
    let local_alerts_uri: axum::http::Uri = "/alerts?scope=local".parse().expect("valid uri");
    let sig = internal_auth::sign_request(ca_key_pem, &Method::GET, &local_alerts_uri)
        .map_err(ApiError::internal)?;

    for node in nodes {
        if node.node_id == local_node_id {
            continue;
        }
        let base = node.api_base_url.trim_end_matches('/');
        if base.is_empty() {
            unreachable_nodes.push(node.node_id);
            continue;
        }
        let url = format!("{base}/api/admin/alerts?scope=local");
        let request = client
            .get(url)
            .header(
                header::HeaderName::from_static(internal_auth::INTERNAL_SIGNATURE_HEADER),
                sig.clone(),
            )
            .send();
        let response = tokio::time::timeout(Duration::from_secs(3), request).await;
        let response = match response {
            Ok(Ok(response)) => response,
            _ => {
                unreachable_nodes.push(node.node_id);
                continue;
            }
        };

        if !response.status().is_success() {
            unreachable_nodes.push(node.node_id);
            continue;
        }

        match response.json::<AlertsResponse>().await {
            Ok(remote) => items.extend(remote.items),
            Err(_) => unreachable_nodes.push(node.node_id),
        }
    }

    let partial = !unreachable_nodes.is_empty();
    Ok(Json(AlertsResponse {
        partial,
        unreachable_nodes,
        items,
    }))
}

async fn fallback_not_found() -> ApiError {
    ApiError::not_found("not found")
}

const CSP_HEADER_VALUE: &str = concat!(
    "default-src 'self'; ",
    "base-uri 'self'; ",
    "object-src 'none'; ",
    "frame-ancestors 'none'; ",
    "connect-src 'self'; ",
    "img-src 'self' data: blob:; ",
    "script-src 'self'; ",
    "style-src 'self' 'unsafe-inline'; ",
    "font-src 'self';"
);

fn embedded_content_type(path: &str) -> &'static str {
    match std::path::Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
    {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("json") => "application/json; charset=utf-8",
        Some("map") => "application/json; charset=utf-8",
        Some("woff2") => "font/woff2",
        Some("txt") => "text/plain; charset=utf-8",
        Some("webmanifest") => "application/manifest+json; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn embedded_bytes_response(
    body: &'static [u8],
    content_type: &'static str,
    cache_control: &'static str,
    csp: bool,
) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
    );
    headers.insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    if csp {
        headers.insert(
            header::HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static(CSP_HEADER_VALUE),
        );
    }
    (headers, body).into_response()
}

fn embedded_index_response() -> Response {
    let Some(index) = web_assets::get("index.html") else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    embedded_bytes_response(index, "text/html; charset=utf-8", "no-cache", true)
}

async fn embedded_asset(Path(path): Path<String>) -> Response {
    let key = format!("assets/{path}");
    let Some(asset) = web_assets::get(&key) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    embedded_bytes_response(
        asset,
        embedded_content_type(&key),
        "public, max-age=31536000, immutable",
        false,
    )
}

async fn embedded_spa_fallback(req: Request<Body>) -> Response {
    if !matches!(*req.method(), Method::GET | Method::HEAD) {
        return StatusCode::NOT_FOUND.into_response();
    }

    let path = req.uri().path().trim_start_matches('/');
    if path.is_empty() {
        return embedded_index_response();
    }

    if let Some(bytes) = web_assets::get(path) {
        let cache_control = if path.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        return embedded_bytes_response(
            bytes,
            embedded_content_type(path),
            cache_control,
            path == "index.html",
        );
    }

    embedded_index_response()
}

#[derive(Debug, Deserialize)]
struct SubscriptionQuery {
    format: Option<String>,
}

fn text_plain_utf8(body: String) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        "text/plain; charset=utf-8".parse().unwrap(),
    );
    (headers, body).into_response()
}

fn text_yaml_utf8(body: String) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        "text/yaml; charset=utf-8".parse().unwrap(),
    );
    (headers, body).into_response()
}

async fn get_subscription(
    Extension(state): Extension<AppState>,
    Path(subscription_token): Path<String>,
    axum::extract::Query(query): axum::extract::Query<SubscriptionQuery>,
) -> Result<Response, ApiError> {
    let format = match query.format.as_deref() {
        None => "base64",
        Some("raw") => "raw",
        Some("clash") => "clash",
        Some(_) => {
            return Err(ApiError::invalid_request(
                "invalid format, expected raw|clash or omit for base64",
            ));
        }
    };

    let (user, grants, endpoints, nodes) = {
        let store = state.store.lock().await;
        let user = store
            .get_user_by_subscription_token(&subscription_token)
            .ok_or_else(|| ApiError::not_found("not found"))?;
        let grants: Vec<Grant> = store
            .state()
            .grants
            .values()
            .filter(|g| g.user_id == user.user_id)
            .cloned()
            .collect();
        let endpoints = store.list_endpoints();
        let nodes = store.list_nodes();
        (user, grants, endpoints, nodes)
    };

    match format {
        "raw" => subscription::build_raw_text(&user, &grants, &endpoints, &nodes)
            .map(text_plain_utf8)
            .map_err(|_e| ApiError::internal("failed to build subscription")),
        "base64" => subscription::build_base64(&user, &grants, &endpoints, &nodes)
            .map(text_plain_utf8)
            .map_err(|_e| ApiError::internal("failed to build subscription")),
        "clash" => subscription::build_clash_yaml(&user, &grants, &endpoints, &nodes)
            .map(text_yaml_utf8)
            .map_err(|_e| ApiError::internal("failed to build subscription")),
        _ => Err(ApiError::internal("unreachable subscription format")),
    }
}

#[cfg(test)]
mod tests;
