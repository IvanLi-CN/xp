use std::sync::Arc;

use axum::{
    Json, Router,
    body::Body,
    extract::{Extension, FromRequest, Path, Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::sync::Mutex;

use crate::{
    cluster_identity::JoinToken,
    cluster_metadata::ClusterMetadata,
    config::Config,
    cycle::{CycleWindowError, current_cycle_window_now, effective_cycle_policy_and_day},
    domain::{CyclePolicy, CyclePolicyDefault, Endpoint, EndpointKind, Grant, Node, User},
    raft::{
        app::RaftFacade,
        types::{
            ClientResponse as RaftClientResponse, NodeId as RaftNodeId, NodeMeta as RaftNodeMeta,
            raft_node_id_from_ulid,
        },
    },
    reconcile::ReconcileHandle,
    state::{JsonSnapshotStore, StoreError},
    subscription, xray,
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub store: Arc<Mutex<JsonSnapshotStore>>,
    pub reconcile: ReconcileHandle,
    pub cluster: Arc<ClusterMetadata>,
    pub cluster_ca_pem: Arc<String>,
    pub cluster_ca_key_pem: Arc<Option<String>>,
    pub raft: Arc<dyn RaftFacade>,
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

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal", StatusCode::INTERNAL_SERVER_ERROR, message)
    }
}

impl From<StoreError> for ApiError {
    fn from(value: StoreError) -> Self {
        match value {
            StoreError::Domain(domain) => match domain {
                crate::domain::DomainError::MissingUser { .. }
                | crate::domain::DomainError::MissingEndpoint { .. } => {
                    ApiError::not_found(domain.to_string())
                }
                _ => ApiError::invalid_request(domain.to_string()),
            },
            StoreError::SchemaVersionMismatch { .. } => ApiError::internal(value.to_string()),
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
    public_domain: String,
    api_base_url: String,
    csr_pem: String,
}

#[derive(Serialize)]
struct ClusterJoinResponse {
    node_id: String,
    signed_cert_pem: String,
    cluster_ca_pem: String,
    cluster_ca_key_pem: String,
}

#[derive(Deserialize)]
struct CreateUserRequest {
    display_name: String,
    cycle_policy_default: CyclePolicyDefault,
    cycle_day_of_month_default: u8,
}

#[derive(Deserialize)]
struct CreateGrantRequest {
    user_id: String,
    endpoint_id: String,
    quota_limit_bytes: u64,
    cycle_policy: CyclePolicy,
    cycle_day_of_month: Option<u8>,
    note: Option<String>,
}

#[derive(Deserialize)]
struct PatchGrantRequest {
    enabled: bool,
    quota_limit_bytes: u64,
    cycle_policy: CyclePolicy,
    cycle_day_of_month: Option<u8>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
struct RealityConfig {
    dest: String,
    server_names: Vec<String>,
    fingerprint: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CreateEndpointRequest {
    VlessRealityVisionTcp {
        node_id: String,
        port: u16,
        public_domain: String,
        reality: RealityConfig,
    },
    #[serde(rename = "ss2022_2022_blake3_aes_128_gcm")]
    Ss2022_2022Blake3Aes128Gcm { node_id: String, port: u16 },
}

#[allow(clippy::too_many_arguments)]
pub fn build_router(
    config: Config,
    store: Arc<Mutex<JsonSnapshotStore>>,
    reconcile: ReconcileHandle,
    cluster: ClusterMetadata,
    cluster_ca_pem: String,
    cluster_ca_key_pem: Option<String>,
    raft: Arc<dyn RaftFacade>,
    raft_rpc: Option<openraft::Raft<crate::raft::types::TypeConfig>>,
) -> Router {
    let app_state = AppState {
        config: Arc::new(config),
        store,
        reconcile,
        cluster: Arc::new(cluster),
        cluster_ca_pem: Arc::new(cluster_ca_pem),
        cluster_ca_key_pem: Arc::new(cluster_ca_key_pem),
        raft,
    };

    let admin_token = app_state.config.admin_token.clone();

    let admin = Router::new()
        .route("/cluster/join-tokens", post(admin_create_join_token))
        .route("/nodes", get(admin_list_nodes))
        .route("/nodes/:node_id", get(admin_get_node))
        .route(
            "/endpoints",
            post(admin_create_endpoint).get(admin_list_endpoints),
        )
        .route(
            "/endpoints/:endpoint_id",
            get(admin_get_endpoint).delete(admin_delete_endpoint),
        )
        .route(
            "/endpoints/:endpoint_id/rotate-shortid",
            post(admin_rotate_short_id),
        )
        .route("/users", post(admin_create_user).get(admin_list_users))
        .route(
            "/users/:user_id",
            get(admin_get_user).delete(admin_delete_user),
        )
        .route("/users/:user_id/reset-token", post(admin_reset_user_token))
        .route("/grants", post(admin_create_grant).get(admin_list_grants))
        .route(
            "/grants/:grant_id",
            get(admin_get_grant)
                .delete(admin_delete_grant)
                .patch(admin_patch_grant),
        )
        .route("/grants/:grant_id/usage", get(admin_get_grant_usage))
        .layer(middleware::from_fn_with_state(admin_token, admin_auth));

    let mut app = Router::new()
        .route("/api/health", get(health))
        .route("/api/cluster/info", get(cluster_info))
        .route("/api/cluster/join", post(cluster_join))
        .route("/api/sub/:subscription_token", get(get_subscription))
        .nest("/api/admin", admin)
        .fallback(fallback_not_found);

    if let Some(raft) = raft_rpc {
        app = app.merge(crate::raft::http_rpc::build_raft_rpc_router(
            crate::raft::http_rpc::RaftRpcState { raft },
        ));
    }

    app.layer(middleware::from_fn(redirect_follower_writes))
        .layer(Extension(app_state))
}

async fn admin_auth(
    State(expected_token): State<String>,
    req: Request<Body>,
    next: Next,
) -> Response {
    match extract_bearer_token(req.headers()) {
        Some(token) if token == expected_token => next.run(req).await,
        _ => ApiError::unauthorized("missing or invalid authorization token").into_response(),
    }
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::AUTHORIZATION)?;
    let raw = raw.to_str().ok()?;
    let raw = raw.strip_prefix("Bearer ")?;
    Some(raw.to_string())
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
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
    let is_admin_write = path.starts_with("/api/admin") && is_write;

    if !is_write || (!is_cluster_write && !is_admin_write) {
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
    }))
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

    let signed_cert_pem = crate::cluster_identity::sign_node_csr(
        &state.cluster.cluster_id,
        &ca_key_pem,
        &req.csr_pem,
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let node = Node {
        node_id: node_id.clone(),
        node_name: req.node_name.clone(),
        public_domain: req.public_domain.clone(),
        api_base_url: req.api_base_url.clone(),
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

    Ok(Json(ClusterJoinResponse {
        node_id,
        signed_cert_pem,
        cluster_ca_pem: (*state.cluster_ca_pem).clone(),
        cluster_ca_key_pem: ca_key_pem,
    }))
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

async fn admin_create_endpoint(
    Extension(state): Extension<AppState>,
    ApiJson(req): ApiJson<CreateEndpointRequest>,
) -> Result<Json<Endpoint>, ApiError> {
    let (node_id, kind, port, meta) = match req {
        CreateEndpointRequest::VlessRealityVisionTcp {
            node_id,
            port,
            public_domain,
            reality,
        } => (
            node_id,
            crate::domain::EndpointKind::VlessRealityVisionTcp,
            port,
            json!({ "public_domain": public_domain, "reality": reality }),
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
        store.build_user(
            req.display_name,
            req.cycle_policy_default,
            req.cycle_day_of_month_default,
        )?
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

async fn admin_create_grant(
    Extension(state): Extension<AppState>,
    ApiJson(req): ApiJson<CreateGrantRequest>,
) -> Result<Json<Grant>, ApiError> {
    let grant = {
        let store = state.store.lock().await;
        store.build_grant(
            req.user_id,
            req.endpoint_id,
            req.quota_limit_bytes,
            req.cycle_policy,
            req.cycle_day_of_month,
            req.note,
        )?
    };
    let _ = raft_write(
        &state,
        crate::state::DesiredStateCommand::UpsertGrant {
            grant: grant.clone(),
        },
    )
    .await?;
    state.reconcile.request_full();
    Ok(Json(grant))
}

async fn admin_patch_grant(
    Extension(state): Extension<AppState>,
    Path(grant_id): Path<String>,
    ApiJson(req): ApiJson<PatchGrantRequest>,
) -> Result<Json<Grant>, ApiError> {
    let out = raft_write(
        &state,
        crate::state::DesiredStateCommand::UpdateGrantFields {
            grant_id: grant_id.clone(),
            enabled: req.enabled,
            quota_limit_bytes: req.quota_limit_bytes,
            cycle_policy: req.cycle_policy,
            cycle_day_of_month: req.cycle_day_of_month,
        },
    )
    .await?;
    let crate::state::DesiredStateApplyResult::GrantUpdated { grant } = out else {
        return Err(ApiError::internal("unexpected raft apply result"));
    };
    let grant = grant.ok_or_else(|| ApiError::not_found(format!("grant not found: {grant_id}")))?;
    state.reconcile.request_full();
    Ok(Json(grant))
}

async fn admin_list_grants(
    Extension(state): Extension<AppState>,
) -> Result<Json<Items<Grant>>, ApiError> {
    let store = state.store.lock().await;
    Ok(Json(Items {
        items: store.list_grants(),
    }))
}

async fn admin_get_grant(
    Extension(state): Extension<AppState>,
    Path(grant_id): Path<String>,
) -> Result<Json<Grant>, ApiError> {
    let store = state.store.lock().await;
    let grant = store
        .get_grant(&grant_id)
        .ok_or_else(|| ApiError::not_found(format!("grant not found: {grant_id}")))?;
    Ok(Json(grant))
}

async fn admin_delete_grant(
    Extension(state): Extension<AppState>,
    Path(grant_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let (endpoint_tag, email) = {
        let store = state.store.lock().await;
        let grant = store
            .get_grant(&grant_id)
            .ok_or_else(|| ApiError::not_found(format!("grant not found: {grant_id}")))?;
        let email = format!("grant:{grant_id}");
        let endpoint_tag = store.get_endpoint(&grant.endpoint_id).map(|e| e.tag);
        (endpoint_tag, email)
    };

    let out = raft_write(
        &state,
        crate::state::DesiredStateCommand::DeleteGrant {
            grant_id: grant_id.clone(),
        },
    )
    .await?;
    let crate::state::DesiredStateApplyResult::GrantDeleted { deleted } = out else {
        return Err(ApiError::internal("unexpected raft apply result"));
    };
    if !deleted {
        return Err(ApiError::not_found(format!("grant not found: {grant_id}")));
    }

    if let Some(tag) = endpoint_tag {
        state.reconcile.request_remove_user(tag, email);
    }
    state.reconcile.request_full();
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct GrantUsageResponse {
    grant_id: String,
    cycle_start_at: String,
    cycle_end_at: String,
    used_bytes: u64,
}

fn map_cycle_window_error(err: CycleWindowError) -> ApiError {
    match err {
        CycleWindowError::UserNotFound { user_id } => ApiError::not_found(format!(
            "user not found (cycle_policy=inherit_user): {user_id}"
        )),
        CycleWindowError::MissingCycleDayOfMonth => {
            ApiError::invalid_request("cycle_day_of_month is required")
        }
        _ => ApiError::internal(err.to_string()),
    }
}

async fn admin_get_grant_usage(
    Extension(state): Extension<AppState>,
    Path(grant_id): Path<String>,
) -> Result<Json<GrantUsageResponse>, ApiError> {
    let (grant, policy, day_of_month) = {
        let store = state.store.lock().await;
        let grant = store
            .get_grant(&grant_id)
            .ok_or_else(|| ApiError::not_found(format!("grant not found: {grant_id}")))?;
        let (policy, day_of_month) =
            effective_cycle_policy_and_day(&store, &grant).map_err(map_cycle_window_error)?;
        (grant, policy, day_of_month)
    };

    let (cycle_start, cycle_end) =
        current_cycle_window_now(policy, day_of_month).map_err(map_cycle_window_error)?;
    let cycle_start_at = cycle_start.to_rfc3339();
    let cycle_end_at = cycle_end.to_rfc3339();

    let email = format!("grant:{grant_id}");
    let (uplink_total, downlink_total) = {
        let mut client = xray::connect(state.config.xray_api_addr)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        client
            .get_user_traffic_totals(&email)
            .await
            .map_err(|status| ApiError::internal(format!("xray get_stats failed: {status}")))?
    };
    let seen_at = Utc::now().to_rfc3339();

    let snapshot = {
        let mut store = state.store.lock().await;
        store.apply_grant_usage_sample(
            &grant.grant_id,
            cycle_start_at.clone(),
            cycle_end_at.clone(),
            uplink_total,
            downlink_total,
            seen_at,
        )?
    };

    Ok(Json(GrantUsageResponse {
        grant_id,
        cycle_start_at: snapshot.cycle_start_at,
        cycle_end_at: snapshot.cycle_end_at,
        used_bytes: snapshot.used_bytes,
    }))
}

async fn fallback_not_found() -> ApiError {
    ApiError::not_found("not found")
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
