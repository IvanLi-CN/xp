use std::{sync::Arc, time::Duration};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeshProxyStatus {
    Disabled,
    Ready,
    Fallback,
    Degraded,
}

impl MeshProxyStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Ready => "ready",
            Self::Fallback => "fallback",
            Self::Degraded => "degraded",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshProxySnapshot {
    pub status: MeshProxyStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_fallback_at: Option<String>,
}

#[derive(Clone)]
pub struct MeshProxyStateHandle {
    inner: Arc<Mutex<MeshProxySnapshot>>,
}

impl MeshProxyStateHandle {
    pub fn disabled() -> Self {
        Self {
            inner: Arc::new(Mutex::new(MeshProxySnapshot {
                status: MeshProxyStatus::Disabled,
                fallback_reason: None,
                last_fallback_at: None,
            })),
        }
    }

    pub fn ready() -> Self {
        Self {
            inner: Arc::new(Mutex::new(MeshProxySnapshot {
                status: MeshProxyStatus::Ready,
                fallback_reason: None,
                last_fallback_at: None,
            })),
        }
    }

    pub async fn snapshot(&self) -> MeshProxySnapshot {
        self.inner.lock().await.clone()
    }

    pub async fn mark_ready(&self) {
        let mut inner = self.inner.lock().await;
        inner.status = MeshProxyStatus::Ready;
        inner.fallback_reason = None;
        inner.last_fallback_at = None;
    }

    pub async fn mark_fallback(&self, reason: impl Into<String>) {
        let mut inner = self.inner.lock().await;
        inner.status = MeshProxyStatus::Fallback;
        inner.fallback_reason = Some(reason.into());
        inner.last_fallback_at = Some(Utc::now().to_rfc3339());
    }

    pub async fn mark_degraded(&self, reason: impl Into<String>) {
        let mut inner = self.inner.lock().await;
        inner.status = MeshProxyStatus::Degraded;
        inner.fallback_reason = Some(reason.into());
        inner.last_fallback_at = Some(Utc::now().to_rfc3339());
    }
}

#[derive(Debug)]
pub enum MeshProxyError {
    InvalidProxyUrl { proxy_url: String, message: String },
}

impl std::fmt::Display for MeshProxyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidProxyUrl { proxy_url, message } => {
                write!(f, "invalid proxy url {proxy_url}: {message}")
            }
        }
    }
}

impl std::error::Error for MeshProxyError {}

pub fn apply_optional_proxy(
    builder: reqwest::ClientBuilder,
    proxy_url: Option<&str>,
) -> Result<reqwest::ClientBuilder, MeshProxyError> {
    let Some(proxy_url) = proxy_url.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(builder);
    };

    let proxy = reqwest::Proxy::all(proxy_url).map_err(|err| MeshProxyError::InvalidProxyUrl {
        proxy_url: proxy_url.to_string(),
        message: err.to_string(),
    })?;
    Ok(builder.proxy(proxy))
}

#[derive(Clone)]
pub struct MeshAwareHttpClient {
    direct: reqwest::Client,
    relay: Option<reqwest::Client>,
    state: MeshProxyStateHandle,
}

impl MeshAwareHttpClient {
    pub fn new(
        direct: reqwest::Client,
        relay: Option<reqwest::Client>,
        state: MeshProxyStateHandle,
    ) -> Self {
        Self {
            direct,
            relay,
            state,
        }
    }

    pub fn direct(&self) -> &reqwest::Client {
        &self.direct
    }

    pub fn relay_enabled(&self) -> bool {
        self.relay.is_some()
    }

    pub fn state(&self) -> MeshProxyStateHandle {
        self.state.clone()
    }

    pub async fn send_with_fallback<F>(
        &self,
        budget: Duration,
        build_request: F,
    ) -> Result<reqwest::Response, reqwest::Error>
    where
        F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    {
        if let Some(relay) = &self.relay {
            let relay_budget =
                std::cmp::min(budget / 2, Duration::from_secs(1)).max(Duration::from_millis(1));
            match tokio::time::timeout(relay_budget, build_request(relay).send()).await {
                Ok(Ok(response)) => {
                    self.state.mark_ready().await;
                    return Ok(response);
                }
                Err(_) => {
                    let relay_reason = format!("relay timed out after {:?}", relay_budget);
                    tracing::warn!(
                        target = "xp::control_plane_mesh",
                        error = %relay_reason,
                        "control-plane relay request failed; falling back to direct"
                    );

                    match build_request(&self.direct).send().await {
                        Ok(response) => {
                            self.state.mark_fallback(relay_reason).await;
                            return Ok(response);
                        }
                        Err(direct_err) => {
                            self.state
                                .mark_degraded(format!(
                                    "relay timed out; direct failed: {direct_err}"
                                ))
                                .await;
                            return Err(direct_err);
                        }
                    }
                }
                Ok(Err(relay_err)) => {
                    let relay_reason = relay_err.to_string();
                    tracing::warn!(
                        target = "xp::control_plane_mesh",
                        error = %relay_reason,
                        "control-plane relay request failed; falling back to direct"
                    );

                    match build_request(&self.direct).send().await {
                        Ok(response) => {
                            self.state.mark_fallback(relay_reason).await;
                            return Ok(response);
                        }
                        Err(direct_err) => {
                            self.state
                                .mark_degraded(format!(
                                    "relay failed: {relay_reason}; direct failed: {direct_err}"
                                ))
                                .await;
                            return Err(direct_err);
                        }
                    }
                }
            }
        }

        build_request(&self.direct).send().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_proxy_status_strings_are_stable() {
        assert_eq!(MeshProxyStatus::Disabled.as_str(), "disabled");
        assert_eq!(MeshProxyStatus::Ready.as_str(), "ready");
        assert_eq!(MeshProxyStatus::Fallback.as_str(), "fallback");
        assert_eq!(MeshProxyStatus::Degraded.as_str(), "degraded");
    }

    #[test]
    fn invalid_proxy_url_is_rejected() {
        let builder = reqwest::Client::builder();
        let err = apply_optional_proxy(builder, Some("not a url")).unwrap_err();
        assert!(err.to_string().contains("invalid proxy url"));
    }
}
