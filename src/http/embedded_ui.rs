use std::io::Read;

use axum::{
    body::Body,
    extract::{Path, Request},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use flate2::read::GzDecoder;

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
    asset: web_assets::EmbeddedAsset,
    content_type: &'static str,
    cache_policy: EmbeddedCachePolicy,
    csp: bool,
    accepts_gzip: bool,
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
    let body = if asset.gzip && accepts_gzip {
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        Body::from(asset.bytes)
    } else if asset.gzip {
        let mut decoder = GzDecoder::new(asset.bytes);
        let mut decoded = Vec::new();
        if decoder.read_to_end(&mut decoded).is_err() {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        Body::from(decoded)
    } else {
        Body::from(asset.bytes)
    };
    if asset.gzip {
        headers.insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
    }
    if csp {
        headers.insert(
            header::HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static(CSP_HEADER_VALUE),
        );
    }
    (headers, body).into_response()
}

fn client_accepts_gzip(headers: &HeaderMap) -> bool {
    let mut gzip = None;
    let mut wildcard = None;
    for value in headers.get_all(header::ACCEPT_ENCODING) {
        let Ok(value) = value.to_str() else {
            continue;
        };
        for entry in value.split(',') {
            let mut parts = entry.split(';');
            let coding = parts.next().unwrap_or_default().trim();
            let quality = parts
                .find_map(|part| part.trim().strip_prefix("q="))
                .and_then(|quality| quality.parse::<f32>().ok())
                .unwrap_or(1.0);
            if coding.eq_ignore_ascii_case("gzip") {
                gzip = Some(quality);
            } else if coding == "*" {
                wildcard = Some(quality);
            }
        }
    }
    gzip.or(wildcard).is_some_and(|quality| quality > 0.0)
}

fn embedded_index_response(accepts_gzip: bool) -> Response {
    let Some(index) = web_assets::get("index.html") else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    embedded_bytes_response(
        index,
        "text/html; charset=utf-8",
        EMBEDDED_NO_CACHE_POLICY,
        true,
        accepts_gzip,
    )
}

pub async fn embedded_asset(Path(path): Path<String>, headers: HeaderMap) -> Response {
    let key = format!("assets/{path}");
    let Some(asset) = web_assets::get(&key) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    embedded_bytes_response(
        asset,
        embedded_content_type(&key),
        EMBEDDED_IMMUTABLE_CACHE_POLICY,
        false,
        client_accepts_gzip(&headers),
    )
}

pub async fn embedded_spa_fallback(req: Request<Body>) -> Response {
    if !matches!(*req.method(), Method::GET | Method::HEAD) {
        return StatusCode::NOT_FOUND.into_response();
    }

    let accepts_gzip = client_accepts_gzip(req.headers());
    let path = req.uri().path().trim_start_matches('/');
    if path.is_empty() {
        return embedded_index_response(accepts_gzip);
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
            accepts_gzip,
        );
    }

    embedded_index_response(accepts_gzip)
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::{client_accepts_gzip, embedded_spa_fallback};
    use axum::{
        body::Body,
        http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    };
    use flate2::read::GzDecoder;
    use http_body_util::BodyExt;

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
        assert!(headers.get(header::CONTENT_ENCODING).is_none());
        assert_eq!(headers.get(header::VARY).unwrap(), "Accept-Encoding");
    }

    #[tokio::test]
    async fn embedded_index_uses_gzip_when_accepted() {
        let request = axum::extract::Request::builder()
            .method(Method::GET)
            .uri("/")
            .header(header::ACCEPT_ENCODING, "br, GZIP")
            .body(Body::empty())
            .unwrap();
        let response = embedded_spa_fallback(request).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_ENCODING).unwrap(),
            "gzip"
        );
        assert_eq!(
            response.headers().get(header::VARY).unwrap(),
            "Accept-Encoding"
        );

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let mut decoder = GzDecoder::new(body.as_ref());
        let mut decoded = String::new();
        decoder.read_to_string(&mut decoded).unwrap();
        assert!(decoded.contains("<!doctype html>"));
    }

    #[test]
    fn explicit_gzip_rejection_overrides_wildcard() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_static("gzip;q=0, *;q=1"),
        );
        assert!(!client_accepts_gzip(&headers));
    }
}
