use std::time::Duration;

use anyhow::Context;
use reqwest::Client;

use crate::{internal_auth, node_history::NodeHistorySnapshot};

const REMOTE_SYNC_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) struct RemoteSyncAuth<'a> {
    cluster_id: &'a str,
    sender_id: &'a str,
    cluster_ca_key_pem: &'a str,
    cluster_ca_pem: &'a str,
}

impl<'a> RemoteSyncAuth<'a> {
    pub(super) fn new(
        cluster_id: &'a str,
        sender_id: &'a str,
        cluster_ca_key_pem: &'a str,
        cluster_ca_pem: &'a str,
    ) -> Self {
        Self {
            cluster_id,
            sender_id,
            cluster_ca_key_pem,
            cluster_ca_pem,
        }
    }
}

fn signed_remote_headers(
    auth: &RemoteSyncAuth<'_>,
    target_id: &str,
    method: axum::http::Method,
    uri: axum::http::Uri,
) -> anyhow::Result<axum::http::HeaderMap> {
    let context = internal_auth::RequestContext::now(
        internal_auth::InternalRoute::MeshV2,
        auth.cluster_id,
        auth.sender_id,
        target_id,
        crate::id::new_ulid_string(),
    );
    let mut headers = axum::http::HeaderMap::new();
    internal_auth::sign_request_v2(
        auth.cluster_ca_key_pem,
        auth.cluster_ca_pem,
        &method,
        &uri,
        None,
        &[],
        &context,
        &mut headers,
    )
    .map_err(|error| anyhow::anyhow!("sign internal request: {error}"))?;
    Ok(headers)
}

pub(super) async fn clear_node_history(
    client: &Client,
    auth: &RemoteSyncAuth<'_>,
    base: &str,
    target_id: &str,
    node_id: &str,
) -> anyhow::Result<()> {
    let uri: axum::http::Uri = format!("/api/admin/_internal/nodes/{node_id}/history")
        .parse()
        .context("invalid node history cleanup path")?;
    let headers = signed_remote_headers(auth, target_id, axum::http::Method::DELETE, uri)?;
    let response = tokio::time::timeout(
        REMOTE_SYNC_TIMEOUT,
        client
            .delete(format!(
                "{base}/api/admin/_internal/nodes/{node_id}/history"
            ))
            .headers(headers)
            .send(),
    )
    .await
    .context("node history cleanup request timeout")??;
    if !response.status().is_success() {
        anyhow::bail!("node history cleanup request failed: {}", response.status());
    }
    Ok(())
}

pub(super) async fn clear_user_traffic(
    client: &Client,
    auth: &RemoteSyncAuth<'_>,
    base: &str,
    target_id: &str,
    user_id: &str,
) -> anyhow::Result<()> {
    let uri: axum::http::Uri = format!("/api/admin/_internal/users/{user_id}/traffic/local")
        .parse()
        .context("invalid user traffic cleanup path")?;
    let headers = signed_remote_headers(auth, target_id, axum::http::Method::DELETE, uri)?;
    let response = tokio::time::timeout(
        REMOTE_SYNC_TIMEOUT,
        client
            .delete(format!(
                "{base}/api/admin/_internal/users/{user_id}/traffic/local"
            ))
            .headers(headers)
            .send(),
    )
    .await
    .context("user traffic cleanup request timeout")??;
    if !response.status().is_success() {
        anyhow::bail!("user traffic cleanup request failed: {}", response.status());
    }
    Ok(())
}

pub(super) async fn fetch_snapshot(
    client: &Client,
    auth: &RemoteSyncAuth<'_>,
    base: &str,
    target_id: &str,
) -> anyhow::Result<NodeHistorySnapshot> {
    let uri: axum::http::Uri = "/api/admin/_internal/nodes/history/local"
        .parse()
        .expect("valid uri");
    let headers = signed_remote_headers(auth, target_id, axum::http::Method::GET, uri)?;
    let request = client
        .get(format!("{base}/api/admin/_internal/nodes/history/local"))
        .headers(headers)
        .send();
    let response = tokio::time::timeout(REMOTE_SYNC_TIMEOUT, request)
        .await
        .context("request timeout")??;
    if !response.status().is_success() {
        anyhow::bail!("node history request failed: {}", response.status());
    }
    response
        .json::<NodeHistorySnapshot>()
        .await
        .context("decode node history response")
}
