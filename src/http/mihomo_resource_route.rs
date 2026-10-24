use axum::{
    extract::{Extension, Path, Request},
    http::{HeaderValue, header},
    middleware::Next,
    response::Response,
};

use super::AppState;
use crate::mihomo_resources;

pub(super) async fn no_store_response(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

pub(super) async fn no_store_mihomo_import_response(request: Request, next: Next) -> Response {
    let should_disable_cache = request
        .uri()
        .path()
        .ends_with("/subscription-mihomo-import/preview");
    let mut response = next.run(request).await;
    if should_disable_cache {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    response
}

pub(super) async fn get_mihomo_resource(
    Extension(state): Extension<AppState>,
    Path(resource_id): Path<String>,
) -> Response {
    let deadline = mihomo_resources::resource_deadline();
    let Some(cluster_ca_key_pem) = state.cluster_ca_key_pem.as_ref().as_ref() else {
        return mihomo_resources::not_found_response();
    };
    if resource_id.len() != 64 || !resource_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return mihomo_resources::not_found_response();
    }
    let url = loop {
        if tokio::time::Instant::now() >= deadline {
            return mihomo_resources::upstream_timeout_response();
        }
        let (revision, profiles) = {
            let store = match tokio::time::timeout_at(deadline, state.store.lock()).await {
                Ok(store) => store,
                Err(_) => return mihomo_resources::upstream_timeout_response(),
            };
            (
                store.state().mihomo_resource_revision,
                store
                    .state()
                    .user_mihomo_profiles
                    .values()
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        };
        let cached = match tokio::time::timeout_at(
            deadline,
            state
                .mihomo_resource_directory
                .lookup_cached(revision, &resource_id),
        )
        .await
        {
            Ok(cached) => cached,
            Err(_) => return mihomo_resources::upstream_timeout_response(),
        };
        let url = match cached {
            Some(cached) => cached,
            None => match tokio::time::timeout_at(
                deadline,
                state.mihomo_resource_directory.rebuild_and_lookup(
                    cluster_ca_key_pem,
                    revision,
                    &profiles,
                    &resource_id,
                ),
            )
            .await
            {
                Ok(url) => url,
                Err(_) => return mihomo_resources::upstream_timeout_response(),
            },
        };
        let current_revision = {
            let store = match tokio::time::timeout_at(deadline, state.store.lock()).await {
                Ok(store) => store,
                Err(_) => return mihomo_resources::upstream_timeout_response(),
            };
            store.state().mihomo_resource_revision
        };
        if current_revision == revision {
            break url;
        }
    };
    let Some(url) = url else {
        return mihomo_resources::not_found_response();
    };
    mihomo_resources::proxy_resource_with_policy_until(
        url,
        &resource_id,
        &state.mihomo_resource_policy,
        deadline,
    )
    .await
}
