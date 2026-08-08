use std::time::Duration;

use super::{MeshAwareHttpClient, MeshProxyStateHandle, apply_optional_proxy};

pub const MESH_POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
const MESH_POOL_MAX_IDLE_PER_HOST: usize = 1;
const MESH_H2_INITIAL_STREAM_WINDOW_SIZE: u32 = 65_535;
const MESH_H2_INITIAL_CONNECTION_WINDOW_SIZE: u32 = 65_535;

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
        .identity(identity))
}

fn strict_mesh_client(
    builder: reqwest::ClientBuilder,
    policy: MeshTransportPolicy,
) -> anyhow::Result<reqwest::Client> {
    Ok(builder
        .http2_prior_knowledge()
        .http2_initial_stream_window_size(MESH_H2_INITIAL_STREAM_WINDOW_SIZE)
        .http2_initial_connection_window_size(MESH_H2_INITIAL_CONNECTION_WINDOW_SIZE)
        .pool_max_idle_per_host(MESH_POOL_MAX_IDLE_PER_HOST)
        .pool_idle_timeout(policy.pool_idle_timeout)
        .build()?)
}

pub(crate) fn build_unauthenticated_mesh_http_client(
    state: MeshProxyStateHandle,
) -> anyhow::Result<MeshAwareHttpClient> {
    let mesh = strict_mesh_client(reqwest::Client::builder(), MeshTransportPolicy::default())?;
    let public_direct = reqwest::Client::builder().build()?;
    Ok(MeshAwareHttpClient::from_transport_clients(
        mesh,
        public_direct,
        None,
        state,
    ))
}

pub fn build_mesh_http_client(
    cluster_ca_pem: &str,
    node_cert_pem: &str,
    node_key_pem: &str,
    mesh_proxy_url: Option<&str>,
    state: MeshProxyStateHandle,
) -> anyhow::Result<MeshAwareHttpClient> {
    build_mesh_http_client_with_policy(
        cluster_ca_pem,
        node_cert_pem,
        node_key_pem,
        mesh_proxy_url,
        state,
        MeshTransportPolicy::default(),
    )
}

pub(crate) fn build_mesh_http_client_with_policy(
    cluster_ca_pem: &str,
    node_cert_pem: &str,
    node_key_pem: &str,
    mesh_proxy_url: Option<&str>,
    state: MeshProxyStateHandle,
    policy: MeshTransportPolicy,
) -> anyhow::Result<MeshAwareHttpClient> {
    let identity_pem = format!("{node_cert_pem}\n{node_key_pem}");
    let mesh = strict_mesh_client(
        authenticated_client_builder(cluster_ca_pem, &identity_pem)?,
        policy,
    )?;
    let public_direct = authenticated_client_builder(cluster_ca_pem, &identity_pem)?.build()?;
    let public_relay = mesh_proxy_url
        .map(|proxy_url| {
            apply_optional_proxy(
                authenticated_client_builder(cluster_ca_pem, &identity_pem)?,
                Some(proxy_url),
            )?
            .build()
            .map_err(anyhow::Error::from)
        })
        .transpose()?;
    Ok(MeshAwareHttpClient::from_transport_clients(
        mesh,
        public_direct,
        public_relay,
        state,
    ))
}
