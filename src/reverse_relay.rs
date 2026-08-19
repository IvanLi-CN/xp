//! The constrained HTTP/2-over-SOCKS transport used by a Reality Mesh Rendezvous.
//!
//! This is deliberately an HTTP client, not a generic proxy. The caller supplies a validated
//! control-plane URI and the client keeps one HTTP/2 client per derived portal credential.

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::http::{HeaderMap, Method};
use tokio::sync::Mutex;

#[derive(Clone, Default)]
pub struct ReverseRelayRuntime {
    clients: Arc<Mutex<BTreeMap<String, CachedClient>>>,
    replay: Arc<Mutex<BTreeMap<String, Instant>>>,
    health_verified: Arc<Mutex<BTreeMap<String, (u64, Instant)>>>,
}

struct CachedClient {
    client: reqwest::Client,
    last_used: Instant,
}

impl ReverseRelayRuntime {
    const REPLAY_WINDOW: Duration = Duration::from_secs(120);
    const CLIENT_IDLE: Duration = Duration::from_secs(120);
    const MAX_CLIENTS: usize = 64;

    pub async fn mark_health_verified(&self, target_node_id: &str, generation: u64) {
        self.health_verified
            .lock()
            .await
            .insert(target_node_id.to_string(), (generation, Instant::now()));
    }

    pub async fn has_health_verified(&self, target_node_id: &str, generation: u64) -> bool {
        let now = Instant::now();
        let mut health = self.health_verified.lock().await;
        health.retain(|_, (_, verified_at)| now.duration_since(*verified_at) < Self::REPLAY_WINDOW);
        health
            .get(target_node_id)
            .is_some_and(|(current_generation, _)| *current_generation == generation)
    }

    pub async fn has_any_health_verified(&self) -> bool {
        let now = Instant::now();
        let mut health = self.health_verified.lock().await;
        health.retain(|_, (_, verified_at)| now.duration_since(*verified_at) < Self::REPLAY_WINDOW);
        !health.is_empty()
    }

    /// Consumes one signed outer request id on a Rendezvous. A duplicate is rejected so a
    /// response-start failure cannot silently execute the same control-plane operation twice.
    pub async fn accept_request(&self, sender_node_id: &str, request_id: &str) -> bool {
        let now = Instant::now();
        let mut replay = self.replay.lock().await;
        replay.retain(|_, seen_at| now.duration_since(*seen_at) < Self::REPLAY_WINDOW);
        let key = format!("{sender_node_id}\n{request_id}");
        if replay.contains_key(&key) {
            return false;
        }
        replay.insert(key, now);
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn forward(
        &self,
        socks_addr: &str,
        username: &str,
        password: &str,
        origin: &str,
        method: Method,
        uri: &str,
        headers: &HeaderMap,
        body: Vec<u8>,
        budget: Duration,
    ) -> Result<reqwest::Response, ReverseRelayError> {
        if !uri.starts_with('/') || !origin.ends_with(":443") {
            return Err(ReverseRelayError::Invalid("invalid reverse origin or URI"));
        }
        let key = format!("{socks_addr}\n{username}\n{password}");
        let client = {
            let mut clients = self.clients.lock().await;
            let now = Instant::now();
            clients.retain(|_, cached| now.duration_since(cached.last_used) < Self::CLIENT_IDLE);
            if let Some(cached) = clients.get_mut(&key) {
                cached.last_used = now;
                cached.client.clone()
            } else {
                if clients.len() >= Self::MAX_CLIENTS
                    && let Some(oldest_key) = clients
                        .iter()
                        .min_by_key(|(_, cached)| cached.last_used)
                        .map(|(key, _)| key.clone())
                {
                    clients.remove(&oldest_key);
                }
                let proxy_url = format!("socks5h://{}:{}@{}", username, password, socks_addr);
                let proxy = reqwest::Proxy::all(proxy_url)
                    .map_err(|_| ReverseRelayError::Invalid("invalid reverse SOCKS proxy"))?;
                let client = reqwest::Client::builder()
                    .proxy(proxy)
                    .http2_prior_knowledge()
                    .pool_max_idle_per_host(1)
                    .pool_idle_timeout(Some(Duration::from_secs(120)))
                    .build()
                    .map_err(ReverseRelayError::Client)?;
                clients.insert(
                    key,
                    CachedClient {
                        client: client.clone(),
                        last_used: now,
                    },
                );
                client
            }
        };

        let url = format!("http://{origin}{uri}");
        let mut request = client.request(method, url).body(body);
        for (name, value) in headers {
            // Hop-by-hop headers are never forwarded through the constrained relay.
            if name == axum::http::header::CONNECTION
                || name == axum::http::header::PROXY_AUTHORIZATION
                || name == axum::http::header::PROXY_AUTHENTICATE
            {
                continue;
            }
            request = request.header(name, value);
        }
        tokio::time::timeout(budget, request.send())
            .await
            .map_err(|_| ReverseRelayError::Timeout)?
            .map_err(ReverseRelayError::Client)
    }
}

#[derive(Debug)]
pub enum ReverseRelayError {
    Invalid(&'static str),
    Timeout,
    Client(reqwest::Error),
}

impl std::fmt::Display for ReverseRelayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => f.write_str(message),
            Self::Timeout => f.write_str("reverse relay timed out"),
            Self::Client(error) => write!(f, "reverse relay client failed: {error}"),
        }
    }
}

impl std::error::Error for ReverseRelayError {}

#[cfg(test)]
mod tests {
    use super::ReverseRelayRuntime;

    #[tokio::test]
    async fn replay_ids_are_consumed_per_sender() {
        let runtime = ReverseRelayRuntime::default();
        assert!(runtime.accept_request("sender-a", "request-1").await);
        assert!(!runtime.accept_request("sender-a", "request-1").await);
        assert!(runtime.accept_request("sender-b", "request-1").await);
    }
}
