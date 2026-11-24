use std::time::Duration;

use super::{MeshAwareHttpClient, MeshRequestError};

pub const MESH_POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
pub(crate) const PUBLIC_HEALTH_PROBE_BUDGET: Duration = Duration::from_secs(30);
const MESH_POOL_MAX_IDLE_PER_HOST: usize = 1;
const PUBLIC_DIRECT_POOL_MAX_IDLE_PER_HOST: usize = 1;
const PUBLIC_DIRECT_POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const PUBLIC_DIRECT_TCP_KEEPALIVE: Duration = Duration::from_secs(30);
const PUBLIC_DIRECT_HTTP2_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);
const PUBLIC_DIRECT_HTTP2_KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(10);

#[cfg(test)]
#[path = "public_health_budget_tests.rs"]
mod tests;

pub(super) fn join_url(
    base: &str,
    path_and_query: &str,
    require_https: bool,
) -> Result<String, MeshRequestError> {
    if !path_and_query.starts_with('/') {
        return Err(MeshRequestError::InvalidTarget(
            "request path must start with /".to_string(),
        ));
    }
    let base = reqwest::Url::parse(base)
        .map_err(|error| MeshRequestError::InvalidTarget(error.to_string()))?;
    if !matches!(base.scheme(), "https" | "http")
        || (require_https && base.scheme() != "https")
        || base.path() != "/"
        || !base.username().is_empty()
        || base.password().is_some()
        || base.as_str().contains('@')
        || base.query().is_some()
        || base.fragment().is_some()
    {
        return Err(MeshRequestError::InvalidTarget(
            "invalid peer base URL".to_string(),
        ));
    }
    Ok(format!(
        "{}{}",
        base.as_str().trim_end_matches('/'),
        path_and_query
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshTransportPolicy {
    pub pool_idle_timeout: Duration,
}

impl Default for MeshTransportPolicy {
    fn default() -> Self {
        Self {
            pool_idle_timeout: MESH_POOL_IDLE_TIMEOUT,
        }
    }
}

fn authenticated_client_builder(
    cluster_ca_pem: &str,
    identity_pem: &str,
) -> anyhow::Result<reqwest::ClientBuilder> {
    let ca = reqwest::Certificate::from_pem(cluster_ca_pem.as_bytes())?;
    let identity = reqwest::Identity::from_pem(identity_pem.as_bytes())?;
    Ok(reqwest::Client::builder()
        .add_root_certificate(ca)
        .identity(identity)
        .redirect(reqwest::redirect::Policy::none()))
}

fn public_direct_client_builder() -> reqwest::ClientBuilder {
    public_direct_policy(reqwest::Client::builder())
}

fn public_direct_policy(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    // Keep one reusable HTTP/2 connection per public origin. HTTP/1.1 origins still
    // negotiate down automatically through ALPN, and the idle cap keeps their
    // keep-alive sockets bounded as well. A bounded idle timeout avoids reusing
    // sockets that an edge has already closed without a GOAWAY.
    builder
        // Cloudflare rejects requests without a User-Agent with HTTP 403/1010 at the
        // edge before the signed XP request reaches the registered public endpoint.
        .user_agent("xp")
        .redirect(reqwest::redirect::Policy::none())
        .pool_max_idle_per_host(PUBLIC_DIRECT_POOL_MAX_IDLE_PER_HOST)
        .pool_idle_timeout(PUBLIC_DIRECT_POOL_IDLE_TIMEOUT)
        .tcp_keepalive(PUBLIC_DIRECT_TCP_KEEPALIVE)
        .http2_adaptive_window(true)
        .http2_keep_alive_interval(PUBLIC_DIRECT_HTTP2_KEEPALIVE_INTERVAL)
        .http2_keep_alive_timeout(PUBLIC_DIRECT_HTTP2_KEEPALIVE_TIMEOUT)
        .http2_keep_alive_while_idle(true)
}

fn strict_mesh_client(
    builder: reqwest::ClientBuilder,
    policy: MeshTransportPolicy,
) -> anyhow::Result<reqwest::Client> {
    Ok(builder
        .http2_prior_knowledge()
        // Let an active Raft snapshot make progress without reserving a large window per peer.
        .http2_adaptive_window(true)
        .pool_max_idle_per_host(MESH_POOL_MAX_IDLE_PER_HOST)
        .pool_idle_timeout(policy.pool_idle_timeout)
        .build()?)
}

pub(crate) fn build_unauthenticated_mesh_http_client() -> anyhow::Result<MeshAwareHttpClient> {
    let mesh = strict_mesh_client(reqwest::Client::builder(), MeshTransportPolicy::default())?;
    let public_direct = public_direct_client_builder().build()?;
    Ok(MeshAwareHttpClient::from_transport_clients(
        mesh,
        public_direct,
    ))
}

pub fn build_mesh_http_client(
    cluster_ca_pem: &str,
    node_cert_pem: &str,
    node_key_pem: &str,
) -> anyhow::Result<MeshAwareHttpClient> {
    build_mesh_http_client_with_policy(
        cluster_ca_pem,
        node_cert_pem,
        node_key_pem,
        MeshTransportPolicy::default(),
    )
}

pub(crate) fn build_mesh_http_client_with_policy(
    cluster_ca_pem: &str,
    node_cert_pem: &str,
    node_key_pem: &str,
    policy: MeshTransportPolicy,
) -> anyhow::Result<MeshAwareHttpClient> {
    let identity_pem = format!("{node_cert_pem}\n{node_key_pem}");
    let mesh = strict_mesh_client(
        authenticated_client_builder(cluster_ca_pem, &identity_pem)?,
        policy,
    )?;
    let public_direct =
        public_direct_policy(authenticated_client_builder(cluster_ca_pem, &identity_pem)?)
            .build()?;
    let client = MeshAwareHttpClient::from_transport_clients(mesh, public_direct);
    #[cfg(not(test))]
    let client = client.with_direct_validation_required();
    Ok(client)
}
