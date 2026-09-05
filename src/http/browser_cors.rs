use std::collections::BTreeSet;

use axum::{
    body::Body,
    extract::Extension,
    http::{HeaderValue, Method, Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use reqwest::Url;

use super::AppState;

const ALLOW_METHODS: &str = "GET, POST, PUT, PATCH, DELETE, OPTIONS";
const ALLOW_HEADERS: &str = "Authorization, Content-Type, Accept";

pub(super) fn canonical_https_origin(value: &str) -> Option<String> {
    let url = Url::parse(value).ok()?;
    if url.scheme() != "https"
        || (url.path() != "" && url.path() != "/")
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }

    let host = url.host_str()?;
    let rendered_host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let port = url.port().filter(|port| *port != 443);
    Some(match port {
        Some(port) => format!("https://{rendered_host}:{port}"),
        None => format!("https://{rendered_host}"),
    })
}

fn is_api_path(path: &str) -> bool {
    path == "/api" || path.starts_with("/api/")
}

fn canonical_registered_origins<'a, I>(values: I) -> Vec<String>
where
    I: IntoIterator<Item = &'a str>,
{
    values
        .into_iter()
        .filter_map(canonical_https_origin)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) async fn registered_browser_origins(state: &AppState) -> Vec<String> {
    let store = state.store.lock().await;
    canonical_registered_origins(
        store
            .list_nodes()
            .iter()
            .map(|node| node.api_base_url.as_str()),
    )
}

async fn origin_is_registered(state: &AppState, origin: &str) -> bool {
    registered_browser_origins(state)
        .await
        .iter()
        .any(|registered| registered == origin)
}

fn append_cors_headers(response: &mut Response, origin: &str) {
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(origin) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
    }
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static(ALLOW_METHODS),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static(ALLOW_HEADERS),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("600"),
    );
    headers.append(header::VARY, HeaderValue::from_static("Origin"));
    headers.append(
        header::VARY,
        HeaderValue::from_static("Access-Control-Request-Method"),
    );
    headers.append(
        header::VARY,
        HeaderValue::from_static("Access-Control-Request-Headers"),
    );
}

pub(super) async fn browser_cors(
    Extension(state): Extension<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if !is_api_path(request.uri().path()) {
        return next.run(request).await;
    }

    let Some(raw_origin) = request.headers().get(header::ORIGIN) else {
        return next.run(request).await;
    };
    let Ok(raw_origin) = raw_origin.to_str() else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let Some(origin) = canonical_https_origin(raw_origin) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    if !origin_is_registered(&state, &origin).await {
        return StatusCode::FORBIDDEN.into_response();
    }

    if request.method() == Method::OPTIONS {
        let mut response = StatusCode::NO_CONTENT.into_response();
        append_cors_headers(&mut response, &origin);
        return response;
    }

    let mut response = next.run(request).await;
    append_cors_headers(&mut response, &origin);
    response
}

#[cfg(test)]
mod tests {
    use super::{canonical_https_origin, canonical_registered_origins};

    #[test]
    fn canonicalizes_only_https_origins() {
        assert_eq!(
            canonical_https_origin("https://Example.com/"),
            Some("https://example.com".to_string())
        );
        assert_eq!(
            canonical_https_origin("https://example.com:8443"),
            Some("https://example.com:8443".to_string())
        );
        assert_eq!(canonical_https_origin("http://example.com"), None);
        assert_eq!(canonical_https_origin("https://example.com/api"), None);
        assert_eq!(canonical_https_origin("https://example.com?x=1"), None);
        assert_eq!(canonical_https_origin("null"), None);
    }

    #[test]
    fn canonicalizes_registered_origins_with_deduplication() {
        assert_eq!(
            canonical_registered_origins([
                "https://B.example/",
                "http://insecure.example",
                "https://a.example:443",
                "https://b.example",
                "https://a.example/path",
            ]),
            vec![
                "https://a.example".to_string(),
                "https://b.example".to_string()
            ]
        );
    }
}
