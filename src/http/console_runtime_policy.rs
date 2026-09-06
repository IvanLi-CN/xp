use std::{collections::BTreeSet, time::Duration};

use axum::{Json, extract::Extension};
use chrono::{Duration as ChronoDuration, SecondsFormat, Utc};
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    ApiError, AppState, MeshCapabilityProbeResponse, browser_cors::canonical_https_origin,
    capabilities::STATIC_CONSOLE_CAPABILITY, mesh::send_mesh_internal_capability_read,
};

const POLICY_TTL: ChronoDuration = ChronoDuration::minutes(10);
const CAPABILITY_PROBE_BUDGET: Duration = Duration::from_secs(5);
const MAX_CAPABILITY_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize)]
pub(super) struct RuntimePolicyResponse {
    pub policy_id: String,
    pub cluster_id: String,
    pub issued_at: String,
    pub expires_at: String,
    pub api_origins: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CapabilityResponse {
    capabilities: Vec<String>,
}

pub(super) async fn get_runtime_policy(
    Extension(state): Extension<AppState>,
) -> Result<Json<RuntimePolicyResponse>, ApiError> {
    let nodes = {
        let store = state.store.lock().await;
        store.list_nodes()
    };

    let origin_checks = join_all(nodes.into_iter().map(|node| {
        let state = state.clone();
        async move {
            let origin = canonical_https_origin(&node.api_base_url)?;
            let compatible = if node.node_id == state.cluster.node_id {
                local_node_supports_static_console()
            } else {
                remote_node_supports_static_console(&state, &node).await
            };
            compatible.then_some(origin)
        }
    }))
    .await;

    let api_origins = origin_checks
        .into_iter()
        .flatten()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let issued_at = Utc::now();
    let expires_at = issued_at + POLICY_TTL;
    let policy_id = policy_id(&state.cluster.cluster_id, &api_origins);

    Ok(Json(RuntimePolicyResponse {
        policy_id,
        cluster_id: state.cluster.cluster_id.clone(),
        issued_at: issued_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        expires_at: expires_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        api_origins,
    }))
}

fn local_node_supports_static_console() -> bool {
    true
}

async fn remote_node_supports_static_console(state: &AppState, node: &crate::domain::Node) -> bool {
    let response = match send_mesh_internal_capability_read(
        state,
        &state.mesh_client,
        node,
        CAPABILITY_PROBE_BUDGET,
    )
    .await
    {
        Ok(MeshCapabilityProbeResponse::Verified(response)) => response,
        Ok(MeshCapabilityProbeResponse::PredecessorNotFound) | Err(_) => return false,
    };

    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|length| length > MAX_CAPABILITY_RESPONSE_BYTES as u64)
    {
        return false;
    }

    let bytes = match response.bytes().await {
        Ok(bytes) if bytes.len() <= MAX_CAPABILITY_RESPONSE_BYTES => bytes,
        _ => return false,
    };
    serde_json::from_slice::<CapabilityResponse>(&bytes)
        .ok()
        .is_some_and(|body| {
            body.capabilities
                .iter()
                .any(|capability| capability == STATIC_CONSOLE_CAPABILITY)
        })
}

fn policy_id(cluster_id: &str, api_origins: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"xp-static-console-runtime-policy-v1\n");
    hasher.update(cluster_id.as_bytes());
    hasher.update(b"\n");
    for origin in api_origins {
        hasher.update(origin.as_bytes());
        hasher.update(b"\n");
    }
    format!("rcp-{}", hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::policy_id;

    #[test]
    fn policy_id_is_stable_for_same_sorted_origins() {
        let origins = vec![
            "https://a.example".to_string(),
            "https://b.example".to_string(),
        ];
        assert_eq!(
            policy_id("cluster", &origins),
            policy_id("cluster", &origins)
        );
        assert_ne!(
            policy_id("cluster", &origins),
            policy_id("other-cluster", &origins)
        );
    }
}
