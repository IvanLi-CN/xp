use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use axum::{
    Json,
    extract::{Extension, Query},
    http::{HeaderMap, StatusCode, header},
};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::admin_token::verify_admin_token;

use super::{ApiError, AppState};

#[derive(Clone, Default)]
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
pub(super) struct VersionCheckResponse {
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

pub(super) async fn api_version_check(
    Extension(state): Extension<AppState>,
    headers: HeaderMap,
    Query(query): Query<BTreeMap<String, String>>,
) -> Result<Json<VersionCheckResponse>, ApiError> {
    let current_package = crate::version::VERSION.to_string();
    let current_release_tag = format!("v{current_package}");
    let refresh = ["refresh", "force"]
        .into_iter()
        .any(|key| query.get(key).is_some_and(|value| value.trim() == "1"));
    if refresh {
        ensure_refresh_authorized(&state, &headers)?;
    }

    let cached = if refresh {
        None
    } else {
        state.version_check_cache.lock().await.entry.clone()
    };
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
        let fetched_at = Instant::now();
        cache.entry = Some(VersionCheckCacheEntry {
            fetched_at,
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

fn ensure_refresh_authorized(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let Some(token) = extract_bearer_token(headers) else {
        return Err(ApiError::unauthorized(
            "missing or invalid authorization token",
        ));
    };
    let Some(expected) = state.config.admin_token_hash() else {
        return Err(ApiError::unauthorized(
            "missing or invalid authorization token",
        ));
    };

    if verify_admin_token(&token, &expected)
        || crate::login_token::decode_and_validate_login_token_jwt(
            &token,
            Utc::now(),
            expected.as_str(),
            &state.cluster.cluster_id,
        )
        .is_ok()
    {
        return Ok(());
    }

    Err(ApiError::unauthorized(
        "missing or invalid authorization token",
    ))
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::AUTHORIZATION)?;
    let raw = raw.to_str().ok()?;
    let raw = raw.strip_prefix("Bearer ")?;
    Some(raw.to_string())
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
