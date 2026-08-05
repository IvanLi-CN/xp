export const apiCapabilitiesFixture = {
	release_tag: "v3.22.5",
	capabilities: [
		"api.health",
		"api.cluster-info",
		"admin.nodes",
		"admin.users",
		"admin.endpoints",
		"admin.quota-policy",
		"admin.status-events",
		"admin.upgrade",
		"admin.mesh",
		"admin.reality-domains",
		"admin.node-probes",
		"admin.traffic-usage",
		"admin.mihomo-tools",
	],
	fingerprint: {
		"/api/health": ["status"],
		"/api/cluster/info": [
			"cluster_id",
			"node_id",
			"role",
			"leader_api_base_url",
			"term",
		],
	},
} as const;
