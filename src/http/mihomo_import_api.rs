use axum::{
    body::to_bytes,
    extract::{Extension, FromRequest, Path, Request},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, de::DeserializeOwned};

use super::{ApiError, AppState};
use crate::{mihomo_import, subscription};

const REQUEST_BODY_LIMIT: usize = 4 * 1024 * 1024 + 64 * 1024;

pub(super) struct LimitedApiJson<T>(pub T);

impl<S, T> FromRequest<S> for LimitedApiJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let (_, body) = req.into_parts();
        let bytes = to_bytes(body, REQUEST_BODY_LIMIT)
            .await
            .map_err(|_| ApiError::too_large("request body is too large"))?;
        let _ = state;
        let value = serde_json::from_slice::<T>(&bytes)
            .map_err(|error| ApiError::invalid_request(error.to_string()))?;
        Ok(Self(value))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdminMihomoImportPreviewRequest {
    source: String,
    existing_extra_proxies_yaml: String,
}

fn map_import_error(error: mihomo_import::ImportError) -> ApiError {
    match error {
        mihomo_import::ImportError::Invalid(message) => ApiError::invalid_request(message),
        mihomo_import::ImportError::TooLarge(message) => ApiError::too_large(message),
        mihomo_import::ImportError::Upstream(message) => {
            ApiError::new("upstream_error", StatusCode::BAD_GATEWAY, message)
        }
    }
}

pub(super) async fn admin_preview_user_mihomo_import(
    Extension(state): Extension<AppState>,
    Path(user_id): Path<String>,
    LimitedApiJson(req): LimitedApiJson<AdminMihomoImportPreviewRequest>,
) -> Result<Json<mihomo_import::ImportPreview>, ApiError> {
    let (user, memberships, endpoints, nodes) = {
        let store = state.store.lock().await;
        let user = store
            .get_user(&user_id)
            .ok_or_else(|| ApiError::not_found(format!("user not found: {user_id}")))?;
        let memberships = store
            .list_user_access(&user.user_id)
            .map_err(ApiError::from)?;
        (
            user,
            memberships,
            store.list_endpoints(),
            store.list_nodes(),
        )
    };

    let ca_key_pem = state
        .cluster_ca_key_pem
        .as_deref()
        .ok_or_else(|| ApiError::internal("cluster ca key is unavailable"))?;
    let reserved_names = subscription::mihomo_reserved_proxy_names_for_import(
        ca_key_pem,
        &user,
        &memberships,
        &endpoints,
        &nodes,
    )
    .map_err(|error| ApiError::internal(error.to_string()))?;

    let preview = mihomo_import::preview(
        &req.source,
        &req.existing_extra_proxies_yaml,
        &reserved_names,
    )
    .await
    .map_err(map_import_error)?;
    Ok(Json(preview))
}
