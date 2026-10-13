#![allow(clippy::items_after_test_module)]

use std::{
    collections::BTreeMap,
    future::Future,
    io,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, OnceLock},
    time::Duration,
};

use axum::{
    body::Body,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures_util::{Stream, StreamExt as _, stream};
use hmac::{Hmac, Mac};
use reqwest::{Client, Url};
use sha2::Sha256;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

use crate::mihomo_policy::{self, MihomoResourcePolicy, PolicySnapshot};
use crate::{state::UserMihomoProfile, subscription};

const MAX_RESOURCE_BYTES: u64 = 256 * 1024 * 1024;
const RESOURCE_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_REDIRECTS: usize = 5;
const GLOBAL_CONCURRENCY: usize = 32;
const PER_RESOURCE_CONCURRENCY: usize = 4;

pub const FIXED_GEOX_ASSETS: &[(&str, &str)] = &[
    (
        "geoip",
        "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.dat",
    ),
    (
        "geosite",
        "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geosite.dat",
    ),
    (
        "mmdb",
        "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.metadb",
    ),
    (
        "asn",
        "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/GeoLite2-ASN.mmdb",
    ),
];

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
static GLOBAL_LIMIT: OnceLock<Arc<Semaphore>> = OnceLock::new();
static RESOURCE_LIMITS: OnceLock<Mutex<BTreeMap<String, Arc<Semaphore>>>> = OnceLock::new();

#[derive(Default)]
pub struct ResourceDirectoryCache {
    directory: Mutex<Option<CachedResourceDirectory>>,
}

struct CachedResourceDirectory {
    revision: u64,
    resources: BTreeMap<String, Url>,
}

impl ResourceDirectoryCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn lookup_cached(
        &self,
        revision: u64,
        resource_id_value: &str,
    ) -> Option<Option<Url>> {
        let directory = self.directory.lock().await;
        directory
            .as_ref()
            .filter(|directory| directory.revision == revision)
            .map(|directory| directory.resources.get(resource_id_value).cloned())
    }

    pub async fn rebuild_and_lookup(
        &self,
        cluster_ca_key_pem: &str,
        revision: u64,
        profiles: &[UserMihomoProfile],
        resource_id_value: &str,
    ) -> Option<Url> {
        let mut directory = self.directory.lock().await;
        if directory
            .as_ref()
            .is_none_or(|directory| directory.revision != revision)
        {
            directory.replace(CachedResourceDirectory {
                revision,
                resources: build_resource_directory(cluster_ca_key_pem, profiles),
            });
        }
        directory
            .as_ref()
            .and_then(|directory| directory.resources.get(resource_id_value).cloned())
    }
}

pub fn fixed_geox_assets() -> &'static [(&'static str, &'static str)] {
    FIXED_GEOX_ASSETS
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn resource_ids_are_stable_and_profile_scoped() {
        let url = subscription::normalize_mihomo_external_url(FIXED_GEOX_ASSETS[0].1)
            .expect("normalized https url");
        let id = resource_id("cluster-key", &url);
        assert_eq!(id.len(), 64);
        assert_eq!(
            resolve_resource_url("cluster-key", &id, &[]).expect("fixed/profile lookup"),
            url,
        );
        assert!(resolve_resource_url("other-key", &id, &[]).is_none());
    }

    #[test]
    fn normalize_rejects_userinfo_and_non_https() {
        assert!(subscription::normalize_mihomo_external_url("http://example.com/a").is_none());
        assert!(
            subscription::normalize_mihomo_external_url("https://user:pass@example.com/a")
                .is_none()
        );
    }

    #[test]
    fn not_found_response_is_uncacheable() {
        let response = not_found_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL),
            Some(&HeaderValue::from_static("no-store"))
        );
    }

    #[tokio::test]
    async fn proxy_streams_success_and_hides_upstream_error_bodies() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/resource"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-encoding", "gzip")
                    .insert_header("cache-control", "public, max-age=3600")
                    .insert_header("etag", "\"upstream\"")
                    .insert_header("last-modified", "Wed, 21 Oct 2015 07:28:00 GMT")
                    .insert_header("expires", "Wed, 21 Oct 2030 07:28:00 GMT")
                    .set_body_bytes(b"chunk-onechunk-two"),
            )
            .mount(&server)
            .await;
        let response = proxy_resource(
            Url::parse(&format!("{}/resource", server.uri())).unwrap(),
            "stream-test",
            true,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_ENCODING),
            Some(&HeaderValue::from_static("gzip"))
        );
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL),
            Some(&HeaderValue::from_static("no-store"))
        );
        assert!(response.headers().get(header::ETAG).is_none());
        assert!(response.headers().get(header::LAST_MODIFIED).is_none());
        assert!(response.headers().get(header::EXPIRES).is_none());
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            Bytes::from_static(b"chunk-onechunk-two")
        );

        Mock::given(method("GET"))
            .and(path("/error"))
            .respond_with(ResponseTemplate::new(503).set_body_string("sensitive upstream body"))
            .mount(&server)
            .await;
        let response = proxy_resource(
            Url::parse(&format!("{}/error", server.uri())).unwrap(),
            "error-test",
            true,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL),
            Some(&HeaderValue::from_static("no-store"))
        );
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(
            !body
                .windows(b"sensitive".len())
                .any(|window| window == b"sensitive")
        );
    }

    #[test]
    fn declared_payload_limit_is_checked_before_streaming() {
        assert!(declared_payload_too_large(Some(MAX_RESOURCE_BYTES + 1)));
        assert!(!declared_payload_too_large(Some(MAX_RESOURCE_BYTES)));
        assert!(!declared_payload_too_large(None));
    }

    #[tokio::test]
    async fn proxy_follows_five_redirects_but_not_the_sixth() {
        let server = MockServer::start().await;
        for index in 0..5 {
            Mock::given(method("GET"))
                .and(path(format!("/redirect-{index}")))
                .respond_with(
                    ResponseTemplate::new(302)
                        .insert_header("location", format!("/redirect-{}", index + 1)),
                )
                .mount(&server)
                .await;
        }
        Mock::given(method("GET"))
            .and(path("/redirect-5"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;
        let response = proxy_resource(
            Url::parse(&format!("{}/redirect-0", server.uri())).unwrap(),
            "redirect-test",
            true,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        Mock::given(method("GET"))
            .and(path("/redirect-6"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", "/redirect-0"))
            .mount(&server)
            .await;
        let response = proxy_resource(
            Url::parse(&format!("{}/redirect-6", server.uri())).unwrap(),
            "redirect-limit-test",
            true,
        )
        .await;
        assert_eq!(response.status(), StatusCode::LOOP_DETECTED);
    }

    #[tokio::test]
    async fn proxy_blocks_private_targets_when_policy_is_disabled() {
        let response = proxy_resource(
            Url::parse("http://127.0.0.1:9/internal").unwrap(),
            "private-target-test",
            false,
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn resource_directory_cache_reuses_and_invalidates_by_revision() {
        let cache = ResourceDirectoryCache::new();
        let url = subscription::normalize_mihomo_external_url(FIXED_GEOX_ASSETS[0].1).unwrap();
        let id = resource_id("cluster-key", &url);

        assert!(cache.lookup_cached(1, &id).await.is_none());
        assert_eq!(
            cache.rebuild_and_lookup("cluster-key", 1, &[], &id).await,
            Some(url.clone())
        );
        assert_eq!(cache.lookup_cached(1, &id).await, Some(Some(url)));
        assert!(cache.lookup_cached(2, &id).await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn request_timeout_maps_to_gateway_timeout() {
        let deadline = Instant::now() + Duration::from_secs(1);
        let result = await_upstream_response(
            deadline,
            std::future::pending::<Result<reqwest::Response, reqwest::Error>>(),
        )
        .await;

        assert_eq!(
            result.err(),
            Some((StatusCode::GATEWAY_TIMEOUT, "upstream_timeout"))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn streaming_timeout_is_reported_after_headers_without_rewriting_status() {
        let global = Arc::new(Semaphore::new(1));
        let resource = Arc::new(Semaphore::new(1));
        let global_permit = global.clone().try_acquire_owned().unwrap();
        let resource_permit = resource.clone().try_acquire_owned().unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut body = Box::pin(limited_stream(
            futures_util::stream::once(async {
                Ok::<Bytes, reqwest::Error>(Bytes::from_static(b"first"))
            })
            .chain(futures_util::stream::pending()),
            deadline,
            global_permit,
            resource_permit,
        ));

        assert_eq!(
            body.next().await.unwrap().unwrap(),
            Bytes::from_static(b"first")
        );
        tokio::time::advance(Duration::from_secs(1)).await;
        let error = body.next().await.unwrap().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(body.next().await.is_none());
        assert_eq!(global.available_permits(), 1);
        assert_eq!(resource.available_permits(), 1);
    }
}

pub fn resource_id(cluster_ca_key_pem: &str, url: &Url) -> String {
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(cluster_ca_key_pem.as_bytes())
        .expect("HMAC accepts arbitrary cluster key bytes");
    mac.update(url.as_str().as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub fn resolve_resource_url(
    cluster_ca_key_pem: &str,
    resource_id_value: &str,
    profiles: &[UserMihomoProfile],
) -> Option<Url> {
    if resource_id_value.len() != 64
        || !resource_id_value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }

    build_resource_directory(cluster_ca_key_pem, profiles).remove(resource_id_value)
}

fn build_resource_directory(
    cluster_ca_key_pem: &str,
    profiles: &[UserMihomoProfile],
) -> BTreeMap<String, Url> {
    let mut resources = BTreeMap::new();

    for (_, raw_url) in FIXED_GEOX_ASSETS {
        if let Some(url) = subscription::normalize_mihomo_external_url(raw_url) {
            resources.insert(resource_id(cluster_ca_key_pem, &url), url);
        }
    }

    for profile in profiles {
        let Ok(urls) = subscription::mihomo_external_resource_urls(profile) else {
            continue;
        };
        for url in urls {
            resources.insert(resource_id(cluster_ca_key_pem, &url), url);
        }
    }
    resources
}

fn build_resource_client() -> Client {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .user_agent("xp-mihomo-resource/1")
        .build()
        .expect("build Mihomo resource client")
}

async fn build_public_resource_client(
    url: &Url,
    deadline: Instant,
    policy: &PolicySnapshot,
) -> Result<Client, (StatusCode, &'static str)> {
    let host = url
        .host_str()
        .ok_or((StatusCode::BAD_GATEWAY, "upstream_unreachable"))?;
    let port = url
        .port_or_known_default()
        .ok_or((StatusCode::BAD_GATEWAY, "upstream_unreachable"))?;
    let lookup = tokio::time::timeout_at(deadline, tokio::net::lookup_host((host, port))).await;
    let addrs: Vec<SocketAddr> = match lookup {
        Ok(Ok(addrs)) => addrs.collect(),
        Ok(Err(_)) => return Err((StatusCode::BAD_GATEWAY, "upstream_unreachable")),
        Err(_) => return Err((StatusCode::GATEWAY_TIMEOUT, "upstream_timeout")),
    };
    let addrs = addrs
        .into_iter()
        .filter(|address| mihomo_policy::is_allowed_address(address.ip(), &policy.effective))
        .collect::<Vec<_>>();
    if addrs.is_empty() {
        return Err((StatusCode::FORBIDDEN, "private_target_blocked"));
    }

    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .user_agent("xp-mihomo-resource/1")
        .resolve_to_addrs(host, &addrs)
        .build()
        .map_err(|_| (StatusCode::BAD_GATEWAY, "upstream_unreachable"))
}

pub async fn proxy_resource(
    url: Url,
    resource_id_value: &str,
    allow_private_targets: bool,
) -> Response {
    let deadline = Instant::now() + RESOURCE_TIMEOUT;
    if allow_private_targets {
        proxy_resource_with_snapshot(url, resource_id_value, None, deadline).await
    } else {
        let snapshot = PolicySnapshot {
            deployment_default: Vec::new(),
            override_cidrs: None,
            effective: Vec::new(),
            source: mihomo_policy::PolicySource::DeploymentDefault,
            status: mihomo_policy::PolicyStatus::Healthy,
            error: None,
        };
        proxy_resource_with_snapshot(url, resource_id_value, Some(snapshot), deadline).await
    }
}

pub async fn proxy_resource_with_policy(
    url: Url,
    resource_id_value: &str,
    policy: &MihomoResourcePolicy,
) -> Response {
    let deadline = Instant::now() + RESOURCE_TIMEOUT;
    let snapshot = match tokio::time::timeout_at(deadline, policy.snapshot()).await {
        Ok(snapshot) => snapshot,
        Err(_) => return error_response(StatusCode::GATEWAY_TIMEOUT, "upstream_timeout"),
    };
    proxy_resource_with_snapshot(url, resource_id_value, Some(snapshot), deadline).await
}

async fn proxy_resource_with_snapshot(
    url: Url,
    resource_id_value: &str,
    policy: Option<PolicySnapshot>,
    deadline: Instant,
) -> Response {
    if Instant::now() >= deadline {
        return error_response(StatusCode::GATEWAY_TIMEOUT, "upstream_timeout");
    }
    let global = GLOBAL_LIMIT
        .get_or_init(|| Arc::new(Semaphore::new(GLOBAL_CONCURRENCY)))
        .clone();
    let global_permit = match global.try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => return error_response(StatusCode::TOO_MANY_REQUESTS, "concurrency_limit"),
    };

    let resource_semaphore = {
        let limits = RESOURCE_LIMITS.get_or_init(|| Mutex::new(BTreeMap::new()));
        let mut limits = match tokio::time::timeout_at(deadline, limits.lock()).await {
            Ok(limits) => limits,
            Err(_) => return error_response(StatusCode::GATEWAY_TIMEOUT, "upstream_timeout"),
        };
        limits.retain(|_, semaphore| {
            semaphore.available_permits() < PER_RESOURCE_CONCURRENCY
                || Arc::strong_count(semaphore) > 1
        });
        limits
            .entry(resource_id_value.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(PER_RESOURCE_CONCURRENCY)))
            .clone()
    };
    if Instant::now() >= deadline {
        return error_response(StatusCode::GATEWAY_TIMEOUT, "upstream_timeout");
    }
    let resource_permit = match resource_semaphore.try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => return error_response(StatusCode::TOO_MANY_REQUESTS, "concurrency_limit"),
    };

    let mut current = url;
    let mut redirects = 0;
    let response = loop {
        if Instant::now() >= deadline {
            return error_response(StatusCode::GATEWAY_TIMEOUT, "upstream_timeout");
        }
        let client = match policy.as_ref() {
            None => CLIENT.get_or_init(build_resource_client).clone(),
            Some(policy) => match build_public_resource_client(&current, deadline, policy).await {
                Ok(client) => client,
                Err((status, code)) => {
                    return error_response(status, code);
                }
            },
        };
        let request = client
            .get(current.clone())
            .header(header::ACCEPT_ENCODING, "identity");
        let response = match await_upstream_response(deadline, request.send()).await {
            Ok(response) => response,
            Err((status, code)) => return error_response(status, code),
        };

        if response.status().is_redirection() {
            if redirects >= MAX_REDIRECTS {
                return error_response(StatusCode::LOOP_DETECTED, "redirect_limit");
            }
            let Some(location) = response.headers().get(header::LOCATION) else {
                return error_response(StatusCode::BAD_GATEWAY, "invalid_redirect");
            };
            let Ok(location) = location.to_str() else {
                return error_response(StatusCode::BAD_GATEWAY, "invalid_redirect");
            };
            let Ok(next) = current.join(location) else {
                return error_response(StatusCode::BAD_GATEWAY, "invalid_redirect");
            };
            if current.scheme() == "https"
                && (next.scheme() != "https"
                    || !next.username().is_empty()
                    || next.password().is_some())
            {
                return error_response(StatusCode::BAD_GATEWAY, "invalid_redirect");
            }
            current = next;
            redirects += 1;
            continue;
        }
        break response;
    };

    let status = response.status();
    if status.is_client_error() || status.is_server_error() {
        return error_response(status, "upstream_error");
    }
    if declared_payload_too_large(response.content_length()) {
        return error_response(StatusCode::PAYLOAD_TOO_LARGE, "resource_too_large");
    }

    let mut builder = Response::builder().status(status);
    builder = builder.header(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    for name in [
        header::CONTENT_TYPE,
        header::CONTENT_LENGTH,
        header::CONTENT_ENCODING,
    ] {
        if let Some(value) = response.headers().get(&name) {
            builder = builder.header(name, value);
        }
    }
    let stream = limited_stream(
        response.bytes_stream(),
        deadline,
        global_permit,
        resource_permit,
    );
    builder
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| error_response(StatusCode::BAD_GATEWAY, "response_build_failed"))
}

fn declared_payload_too_large(length: Option<u64>) -> bool {
    length.is_some_and(|length| length > MAX_RESOURCE_BYTES)
}

type UpstreamStream = Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>;

struct LimitedStream {
    stream: UpstreamStream,
    deadline: Instant,
    seen: u64,
    done: bool,
    _global_permit: Option<OwnedSemaphorePermit>,
    _resource_permit: Option<OwnedSemaphorePermit>,
}

fn limited_stream(
    stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
    deadline: Instant,
    global_permit: OwnedSemaphorePermit,
    resource_permit: OwnedSemaphorePermit,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    let state = LimitedStream {
        stream: Box::pin(stream),
        deadline,
        seen: 0,
        done: false,
        _global_permit: Some(global_permit),
        _resource_permit: Some(resource_permit),
    };
    stream::unfold(state, |mut state| async move {
        if state.done {
            return None;
        }
        if Instant::now() >= state.deadline {
            state.done = true;
            return Some((
                Err(io::Error::new(io::ErrorKind::TimedOut, "upstream timeout")),
                state,
            ));
        }
        match tokio::time::timeout_at(state.deadline, state.stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                state.seen = state.seen.saturating_add(chunk.len() as u64);
                if state.seen > MAX_RESOURCE_BYTES {
                    state.done = true;
                    return Some((
                        Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "resource too large",
                        )),
                        state,
                    ));
                }
                Some((Ok(chunk), state))
            }
            Ok(Some(Err(error))) => {
                state.done = true;
                Some((Err(io::Error::other(error)), state))
            }
            Ok(None) => None,
            Err(_) => {
                state.done = true;
                Some((
                    Err(io::Error::new(io::ErrorKind::TimedOut, "upstream timeout")),
                    state,
                ))
            }
        }
    })
}

async fn await_upstream_response<F>(
    deadline: Instant,
    request: F,
) -> Result<reqwest::Response, (StatusCode, &'static str)>
where
    F: Future<Output = Result<reqwest::Response, reqwest::Error>>,
{
    match tokio::time::timeout_at(deadline, request).await {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(error)) if error.is_timeout() => {
            Err((StatusCode::GATEWAY_TIMEOUT, "upstream_timeout"))
        }
        Ok(Err(_)) => Err((StatusCode::BAD_GATEWAY, "upstream_unreachable")),
        Err(_) => Err((StatusCode::GATEWAY_TIMEOUT, "upstream_timeout")),
    }
}

fn error_response(status: StatusCode, code: &'static str) -> Response {
    let mut response = (status, format!("xp_mihomo_resource_error: {code}\n")).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if status == StatusCode::TOO_MANY_REQUESTS {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    }
    response
}

pub fn not_found_response() -> Response {
    let mut response = (StatusCode::NOT_FOUND, "not found\n").into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
