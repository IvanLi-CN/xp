import type { SubscriptionFormat } from "@/api/subscription";

import type { DemoEndpoint, DemoNode, DemoState, DemoUser } from "./types";

function endpointType(endpoint: DemoEndpoint): string {
	return endpoint.kind === "vless_reality_vision_tcp" ? "vless" : "ss";
}

function endpointHost(endpoint: DemoEndpoint, nodes: DemoNode[]): string {
	return (
		nodes.find((item) => item.id === endpoint.nodeId)?.accessHost ??
		endpoint.nodeId
	);
}

export async function fetchDemoSubscription(
	state: DemoState,
	user: DemoUser,
	format: SubscriptionFormat,
): Promise<string> {
	await new Promise((resolve) => window.setTimeout(resolve, 240));

	const assignedEndpoints = state.endpoints.filter((endpoint) =>
		user.endpointIds.includes(endpoint.id),
	);
	if (assignedEndpoints.length === 0) return "# no endpoint access assigned";

	if (format === "clash" || format === "mihomo") {
		const header =
			format === "mihomo"
				? "# provider mode preview"
				: "# clash-compatible preview";
		return [
			header,
			"proxies:",
			...assignedEndpoints.map(
				(endpoint) =>
					`  - name: ${endpoint.name}\n    type: ${endpointType(endpoint)}\n    server: ${endpointHost(endpoint, state.nodes)}\n    port: ${endpoint.port}`,
			),
			user.mihomoMixinYaml.trim()
				? `# user mixin\n${user.mihomoMixinYaml.trim()}`
				: "# no user mixin",
		].join("\n");
	}

	return assignedEndpoints
		.map(
			(endpoint) =>
				`${endpointType(endpoint)}://${user.subscriptionToken}@${endpointHost(endpoint, state.nodes)}:${endpoint.port}#${endpoint.name}`,
		)
		.join("\n");
}
