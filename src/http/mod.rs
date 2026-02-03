use std::{collections::BTreeSet, sync::Arc};

use axum::{
    Json, Router,
    body::Body,
    extract::{Extension, FromRequest, Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post, put},
};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::{
    sync::Mutex,
    time::{Duration, Instant},
};

use crate::{
    admin_token::{AdminTokenHash, verify_admin_token},
    cluster_identity::JoinToken,
    cluster_metadata::ClusterMetadata,
    config::Config,
    cycle::{CycleTimeZone, current_cycle_window_at},
    domain::{
        Endpoint, EndpointKind, Grant, Node, NodeQuotaReset, QuotaResetSource, User, UserNodeQuota,
        UserQuotaReset, validate_group_name,
    },
    internal_auth,
    protocol::VlessRealityVisionTcpEndpointMeta,
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
    pub cluster: Arc<ClusterMetadata>,
    pub cluster_ca_pem: Arc<String>,
    pub cluster_ca_key_pem: Arc<Option<String>>,
    pub raft: Arc<dyn RaftFacade>,
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
                | crate::domain::DomainError::GrantPairConflict { .. } => {
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
    quota_reset: Option<UserQuotaReset>,
}

#[derive(Deserialize)]
struct PutUserNodeQuotaRequest {
    quota_limit_bytes: u64,
    #[serde(default)]
    quota_reset_source: Option<QuotaResetSource>,
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
    port: Option<u16>,
    #[serde(default, deserialize_with = "deserialize_optional_reality")]
    reality: Option<Option<RealityConfig>>,
}

#[allow(clippy::too_many_arguments)]
pub fn build_router(
    config: Config,
    store: Arc<Mutex<JsonSnapshotStore>>,
    reconcile: ReconcileHandle,
    xray_health: XrayHealthHandle,
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
        cluster: Arc::new(cluster),
        cluster_ca_pem: Arc::new(cluster_ca_pem),
        cluster_ca_key_pem: Arc::new(cluster_ca_key_pem),
        raft,
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
        .route("/cluster/join-tokens", post(admin_create_join_token))
        .route("/config", get(admin_get_config))
        .route("/nodes", get(admin_list_nodes))
        .route(
            "/nodes/:node_id",
            get(admin_get_node).patch(admin_patch_node),
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
            "/grant-groups",
            get(admin_list_grant_groups).post(admin_create_grant_group),
        )
        .route(
            "/grant-groups/:group_name",
            get(admin_get_grant_group)
                .put(admin_replace_grant_group)
                .delete(admin_delete_grant_group),
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
    req: Request<Body>,
    next: Next,
) -> Response {
    if let (Some(sig), Some(ca_key_pem)) = (
        extract_internal_signature(req.headers()),
        auth.cluster_ca_key_pem.as_deref(),
    ) && internal_auth::verify_request(ca_key_pem, req.method(), req.uri(), &sig)
    {
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
        .await
        .map_err(|e| ApiError::new("upstream_error", StatusCode::BAD_GATEWAY, e.to_string()))?;

    if !resp.status().is_success() {
        return Err(ApiError::new(
            "upstream_error",
            StatusCode::BAD_GATEWAY,
            format!("github returned status: {}", resp.status()),
        ));
    }

    let body: GithubLatestReleaseResponse = resp
        .json()
        .await
        .map_err(|e| ApiError::new("upstream_error", StatusCode::BAD_GATEWAY, e.to_string()))?;

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

async fn admin_internal_raft_client_write(
    Extension(state): Extension<AppState>,
    ApiJson(cmd): ApiJson<DesiredStateCommand>,
) -> Result<Json<RaftClientResponse>, ApiError> {
    let resp = state
        .raft
        .client_write(cmd)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(resp))
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
        state.cluster.api_base_url.clone(),
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
                node_name: state.cluster.node_name.clone(),
                access_host: state.cluster.access_host.clone(),
                api_base_url: state.cluster.api_base_url.clone(),
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
    let mut node = {
        let store = state.store.lock().await;
        store
            .get_node(&node_id)
            .ok_or_else(|| ApiError::not_found(format!("node not found: {node_id}")))?
    };

    if let Some(node_name) = req.node_name {
        let Some(node_name) = node_name else {
            return Err(ApiError::invalid_request("node_name cannot be null"));
        };
        node.node_name = node_name;
    }
    if let Some(access_host) = req.access_host {
        let Some(access_host) = access_host else {
            return Err(ApiError::invalid_request("access_host cannot be null"));
        };
        node.access_host = access_host;
    }
    if let Some(api_base_url) = req.api_base_url {
        let Some(api_base_url) = api_base_url else {
            return Err(ApiError::invalid_request("api_base_url cannot be null"));
        };
        node.api_base_url = api_base_url;
    }
    if let Some(quota_reset) = req.quota_reset {
        node.quota_reset = quota_reset;
    }

    let _ = raft_write(
        &state,
        crate::state::DesiredStateCommand::UpsertNode { node: node.clone() },
    )
    .await?;
    Ok(Json(node))
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
) -> Result<Json<Items<Endpoint>>, ApiError> {
    let store = state.store.lock().await;
    Ok(Json(Items {
        items: store.list_endpoints(),
    }))
}

async fn admin_get_endpoint(
    Extension(state): Extension<AppState>,
    Path(endpoint_id): Path<String>,
) -> Result<Json<Endpoint>, ApiError> {
    let store = state.store.lock().await;
    let endpoint = store
        .get_endpoint(&endpoint_id)
        .ok_or_else(|| ApiError::not_found(format!("endpoint not found: {endpoint_id}")))?;
    Ok(Json(endpoint))
}

async fn admin_patch_endpoint(
    Extension(state): Extension<AppState>,
    Path(endpoint_id): Path<String>,
    ApiJson(req): ApiJson<PatchEndpointRequest>,
) -> Result<Json<Endpoint>, ApiError> {
    let mut endpoint = {
        let store = state.store.lock().await;
        store
            .get_endpoint(&endpoint_id)
            .ok_or_else(|| ApiError::not_found(format!("endpoint not found: {endpoint_id}")))?
    };

    if let Some(port) = req.port {
        endpoint.port = port;
    }

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

#[derive(Debug, Serialize, Deserialize, Clone)]
struct AdminUserQuotaSummaryItem {
    user_id: String,
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

fn build_local_user_quota_summaries(
    store: &JsonSnapshotStore,
    local_node_id: &str,
) -> Result<Vec<AdminUserQuotaSummaryItem>, ApiError> {
    let now = Utc::now();

    let endpoints_by_id = store
        .list_endpoints()
        .into_iter()
        .map(|e| (e.endpoint_id.clone(), e))
        .collect::<std::collections::BTreeMap<_, _>>();

    // Collect explicit per-user quota limits for the local node (even if there are no grants yet).
    let mut explicit_limit_by_user_id: std::collections::BTreeMap<String, u64> =
        std::collections::BTreeMap::new();
    for user in store.list_users() {
        if let Some(limit) = store.get_user_node_quota_limit_bytes(&user.user_id, local_node_id) {
            explicit_limit_by_user_id.insert(user.user_id, limit);
        }
    }

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

    let mut items = Vec::new();
    let mut all_user_ids: std::collections::BTreeSet<String> =
        explicit_limit_by_user_id.keys().cloned().collect();
    all_user_ids.extend(grants_by_user.keys().cloned());

    for user_id in all_user_ids {
        let grants = grants_by_user.remove(&user_id).unwrap_or_default();
        let explicit = explicit_limit_by_user_id.get(&user_id).copied();
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

        let remaining_bytes = quota_limit_bytes.saturating_sub(used_bytes);
        items.push(AdminUserQuotaSummaryItem {
            user_id,
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

        match response.json::<AdminUserQuotaSummariesResponse>().await {
            Ok(remote) => {
                for item in remote.items {
                    totals
                        .entry(item.user_id.clone())
                        .and_modify(|entry| {
                            entry.quota_limit_bytes = entry
                                .quota_limit_bytes
                                .saturating_add(item.quota_limit_bytes);
                            entry.used_bytes = entry.used_bytes.saturating_add(item.used_bytes);
                            entry.remaining_bytes =
                                entry.remaining_bytes.saturating_add(item.remaining_bytes);
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
    ApiJson(req): ApiJson<PutUserNodeQuotaRequest>,
) -> Result<Json<UserNodeQuota>, ApiError> {
    let quota_reset_source = match req.quota_reset_source {
        Some(v) => v,
        None => {
            let store = state.store.lock().await;
            store
                .get_user_node_quota_reset_source(&user_id, &node_id)
                .unwrap_or_default()
        }
    };

    let out = raft_write(
        &state,
        crate::state::DesiredStateCommand::SetUserNodeQuota {
            user_id: user_id.clone(),
            node_id: node_id.clone(),
            quota_limit_bytes: req.quota_limit_bytes,
            quota_reset_source,
        },
    )
    .await?;
    let crate::state::DesiredStateApplyResult::UserNodeQuotaSet { quota } = out else {
        return Err(ApiError::internal("unexpected raft apply result"));
    };
    state.reconcile.request_full();
    Ok(Json(quota))
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
