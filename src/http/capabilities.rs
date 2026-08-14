use std::collections::BTreeMap;

use axum::Json;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub(super) struct ApiCapabilitiesResponse {
    release_tag: String,
    capabilities: Vec<&'static str>,
    fingerprint: BTreeMap<&'static str, Vec<&'static str>>,
}

pub(super) async fn api_capabilities() -> Json<ApiCapabilitiesResponse> {
    let mut fingerprint = BTreeMap::new();
    fingerprint.insert("/api/health", vec!["status"]);
    fingerprint.insert(
        "/api/cluster/info",
        vec![
            "cluster_id",
            "node_id",
            "role",
            "leader_api_base_url",
            "term",
        ],
    );
    fingerprint.insert("/api/admin/nodes", vec!["items"]);
    fingerprint.insert("/api/admin/status/events", vec!["hello", "snapshot"]);

    Json(ApiCapabilitiesResponse {
        release_tag: format!("v{}", crate::version::VERSION),
        capabilities: vec![
            "api.health",
            "api.cluster-info",
            "admin.nodes",
            "admin.history-repositories",
            "admin.repository-history",
            "admin.users",
            "admin.endpoints",
            "admin.endpoint-mihomo-smux",
            "admin.endpoint-vless-xhttp",
            "admin.endpoint-conditional-update",
            "admin.alerts",
            "admin.config",
            "admin.quota-policy",
            "admin.status-events",
            "admin.upgrade",
            "admin.mesh",
            "admin.mesh-transport-reuse",
            "admin.reality-domains",
            "admin.node-probes",
            "admin.traffic-usage",
            "admin.mihomo-tools",
            "admin.mihomo-resource-policy",
        ],
        fingerprint,
    })
}

#[cfg(test)]
mod tests {
    use super::api_capabilities;

    #[tokio::test]
    async fn exposes_stable_capability_ids_and_release_tag() {
        let response = api_capabilities().await.0;
        assert_eq!(
            response.release_tag,
            format!("v{}", crate::version::VERSION)
        );
        assert!(response.capabilities.contains(&"api.health"));
        assert!(
            response
                .capabilities
                .contains(&"admin.endpoint-mihomo-smux")
        );
        assert!(
            response
                .capabilities
                .contains(&"admin.endpoint-vless-xhttp")
        );
        assert!(
            response
                .capabilities
                .contains(&"admin.endpoint-conditional-update")
        );
        assert!(response.capabilities.contains(&"admin.status-events"));
        assert!(
            response
                .capabilities
                .contains(&"admin.mesh-transport-reuse")
        );
        assert_eq!(response.fingerprint["/api/health"], vec!["status"]);
    }
}
