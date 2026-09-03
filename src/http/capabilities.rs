use std::collections::BTreeMap;

use axum::Json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub(super) struct ApiCapabilitiesResponse {
    release_tag: String,
    capabilities: Vec<&'static str>,
    fingerprint: BTreeMap<&'static str, Vec<&'static str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) reverse_mesh: Option<ReverseMeshReadiness>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct ReverseMeshReadiness {
    pub xray_ready: bool,
    pub managed_vless_endpoint: bool,
    #[serde(default)]
    pub reverse_ready: bool,
    #[serde(default)]
    pub health_verified: bool,
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

    let mut capabilities = vec![
        "api.health",
        "api.cluster-info",
        "admin.nodes",
        "admin.history-repositories",
        "admin.repository-history",
        "admin.users",
        "admin.endpoints",
        "admin.service-monitors",
        "admin.service-monitor-observer-policy-v1",
        "admin.service-monitor-draft-tests-v1",
        "admin.service-monitor-draft-tests-same-origin-v1",
        "admin.service-monitor-http-v1",
        "admin.service-monitor-tcp-v1",
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
        "admin.mesh-reverse-relay-v1",
        "admin.reality-domains",
        "admin.node-probes",
        "admin.traffic-usage",
        "admin.mihomo-tools",
        "admin.mihomo-resource-policy",
        "node.mihomo-resource-private-cidrs-v1",
        "cluster.join.staged-v1",
        "cluster.membership-lifecycle-v1",
        "cluster.mesh-reverse-assignment-v1",
    ];
    if crate::uptime_runtime::icmp_supported() {
        capabilities.push("admin.service-monitor-icmp-v1");
    }

    Json(ApiCapabilitiesResponse {
        release_tag: format!("v{}", crate::version::VERSION),
        capabilities,
        fingerprint,
        reverse_mesh: None,
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
                .contains(&"admin.service-monitor-draft-tests-same-origin-v1")
        );
        assert!(response.capabilities.contains(&"cluster.join.staged-v1"));
        assert!(
            response
                .capabilities
                .contains(&"cluster.membership-lifecycle-v1")
        );
        assert!(
            response
                .capabilities
                .contains(&"admin.mesh-transport-reuse")
        );
        assert!(
            response
                .capabilities
                .contains(&"admin.mesh-reverse-relay-v1")
        );
        assert!(
            response
                .capabilities
                .contains(&"cluster.mesh-reverse-assignment-v1")
        );
        assert_eq!(response.fingerprint["/api/health"], vec!["status"]);
    }
}
