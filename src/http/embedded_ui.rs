use axum::{
    body::Body,
    extract::{Path, Request},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};

use super::{CSP_HEADER_VALUE, web_assets};

const EMBEDDED_IMMUTABLE_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";
const EMBEDDED_NO_CACHE_CONTROL: &str = "no-cache";
const SERVICE_WORKER_CACHE_CONTROL: &str = "no-cache, no-store, must-revalidate, max-age=0";
const SERVICE_WORKER_CDN_CACHE_CONTROL: &str = "no-store";

#[derive(Clone, Copy)]
struct EmbeddedCachePolicy {
    cache_control: &'static str,
    cdn_cache_control: Option<&'static str>,
    cloudflare_cdn_cache_control: Option<&'static str>,
    pragma: Option<&'static str>,
}

const EMBEDDED_IMMUTABLE_CACHE_POLICY: EmbeddedCachePolicy = EmbeddedCachePolicy {
    cache_control: EMBEDDED_IMMUTABLE_CACHE_CONTROL,
    cdn_cache_control: None,
    cloudflare_cdn_cache_control: None,
    pragma: None,
};

const EMBEDDED_NO_CACHE_POLICY: EmbeddedCachePolicy = EmbeddedCachePolicy {
    cache_control: EMBEDDED_NO_CACHE_CONTROL,
    cdn_cache_control: None,
    cloudflare_cdn_cache_control: None,
    pragma: None,
};

const SERVICE_WORKER_CACHE_POLICY: EmbeddedCachePolicy = EmbeddedCachePolicy {
    cache_control: SERVICE_WORKER_CACHE_CONTROL,
    cdn_cache_control: Some(SERVICE_WORKER_CDN_CACHE_CONTROL),
    cloudflare_cdn_cache_control: Some(SERVICE_WORKER_CDN_CACHE_CONTROL),
    pragma: Some("no-cache"),
};

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
    cache_policy: EmbeddedCachePolicy,
    csp: bool,
) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_policy.cache_control),
    );
    if let Some(value) = cache_policy.cdn_cache_control {
        headers.insert(
            header::HeaderName::from_static("cdn-cache-control"),
            HeaderValue::from_static(value),
        );
    }
    if let Some(value) = cache_policy.cloudflare_cdn_cache_control {
        headers.insert(
            header::HeaderName::from_static("cloudflare-cdn-cache-control"),
            HeaderValue::from_static(value),
        );
    }
    if let Some(value) = cache_policy.pragma {
        headers.insert(header::PRAGMA, HeaderValue::from_static(value));
    }
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
    embedded_bytes_response(
        index,
        "text/html; charset=utf-8",
        EMBEDDED_NO_CACHE_POLICY,
        true,
    )
}

pub async fn embedded_asset(Path(path): Path<String>) -> Response {
    let key = format!("assets/{path}");
    let Some(asset) = web_assets::get(&key) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    embedded_bytes_response(
        asset,
        embedded_content_type(&key),
        EMBEDDED_IMMUTABLE_CACHE_POLICY,
        false,
    )
}

pub async fn embedded_spa_fallback(req: Request<Body>) -> Response {
    if !matches!(*req.method(), Method::GET | Method::HEAD) {
        return StatusCode::NOT_FOUND.into_response();
    }

    let path = req.uri().path().trim_start_matches('/');
    if path.is_empty() {
        return embedded_index_response();
    }

    if let Some(bytes) = web_assets::get(path) {
        let cache_policy = if path == "sw.js" {
            SERVICE_WORKER_CACHE_POLICY
        } else if path.starts_with("assets/") {
            EMBEDDED_IMMUTABLE_CACHE_POLICY
        } else {
            EMBEDDED_NO_CACHE_POLICY
        };
        return embedded_bytes_response(
            bytes,
            embedded_content_type(path),
            cache_policy,
            path == "index.html",
        );
    }

    embedded_index_response()
}

#[cfg(test)]
mod tests {
    use super::embedded_spa_fallback;
    use axum::{
        body::Body,
        http::{Method, StatusCode, header},
    };

    fn req(method: Method, uri: &str) -> axum::extract::Request {
        axum::extract::Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn service_worker_uses_cache_bypass_headers() {
        let response = embedded_spa_fallback(req(Method::GET, "/sw.js")).await;
        assert_eq!(response.status(), StatusCode::OK);

        let headers = response.headers();
        assert_eq!(
            headers.get(header::CONTENT_TYPE).unwrap(),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            headers.get(header::CACHE_CONTROL).unwrap(),
            "no-cache, no-store, must-revalidate, max-age=0"
        );
        assert_eq!(
            headers
                .get(header::HeaderName::from_static("cdn-cache-control"))
                .unwrap(),
            "no-store"
        );
        assert_eq!(
            headers
                .get(header::HeaderName::from_static(
                    "cloudflare-cdn-cache-control"
                ))
                .unwrap(),
            "no-store"
        );
        assert_eq!(headers.get(header::PRAGMA).unwrap(), "no-cache");
    }
}
