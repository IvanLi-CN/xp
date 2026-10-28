import yaml from "js-yaml";

import { createMockApi } from "../../.storybook/mocks/apiMock";
import {
	errorResponse,
	jsonResponse,
	textResponse,
} from "../../.storybook/mocks/mockResponses";
import {
	type AdminUserMihomoProfile,
	AdminUserMihomoProfileSchema,
} from "../api/adminUsers";
import { fixtureCatalog } from "../fixture-policy/catalog";
import { demoMeshStatus } from "./DemoSystemStatusPage";
import { createDemoState } from "./fixtures";
import { renderDemoMihomoSubscription } from "./mihomoModel";
import type { DemoControls } from "./shareState";

export function createDemoApi(controls: DemoControls) {
	const seed = createDemoState(controls.scene);
	const users = seed.users.map((user) => ({
		user_id: user.id,
		display_name: user.displayName,
		subscription_token: user.subscriptionToken,
		credential_epoch: 0,
		priority_tier: user.tier,
		quota_reset: {
			policy: "monthly" as const,
			day_of_month: 1,
			tz_offset_minutes: 480,
		},
	}));
	const profiles = new Map<string, AdminUserMihomoProfile>();
	for (const user of seed.users) {
		const missing =
			controls.profile === "missing" || user.id === "user-fixture-e";
		profiles.set(user.id, {
			mixin_yaml: missing
				? ""
				: yaml.dump({
						"geox-url": { geoip: "https://assets.example.net/geoip.dat" },
						"rule-providers": {
							"demo-rules": {
								type: "http",
								behavior: "domain",
								format: "yaml",
								url:
									controls.profile === "invalid"
										? "http://assets.example.net/rules.yaml"
										: "https://assets.example.net/rules.yaml",
							},
						},
						rules: ["RULE-SET,demo-rules,DIRECT", "MATCH,DIRECT"],
					}),
			extra_proxies_yaml: "",
			extra_proxy_providers_yaml: missing
				? ""
				: yaml.dump({
						"demo-providers": {
							type: "http",
							url: "https://assets.example.net/proxies.yaml",
							path: "./providers/demo.yaml",
							interval: 3600,
						},
					}),
		});
	}
	const mock = createMockApi({
		identityNamespace: "demo",
		trafficNowMs: Date.now(),
		failNodeRuntimeNodeIds:
			controls.scene === "incident" ? [fixtureCatalog.nodeId.fixture36()] : [],
		data: {
			users,
			subscriptions: Object.fromEntries(
				users.map((user) => [
					user.subscription_token,
					fixtureCatalog.subscription.rawUri(),
				]),
			),
			userAccessByUserId: Object.fromEntries(
				users.map((user) => [
					user.user_id,
					[
						{
							user_id: user.user_id,
							endpoint_id: fixtureCatalog.endpointId.fixture40(),
							node_id: fixtureCatalog.nodeId.fixture32(),
						},
					],
				]),
			),
			...(controls.scene === "empty"
				? { endpoints: [], serviceMonitors: [] }
				: {}),
		},
	});
	let mesh = structuredClone(demoMeshStatus);
	return {
		async handle(request: Request): Promise<Response> {
			const url = new URL(request.url);
			const path = url.pathname;
			const method = request.method;
			const userProfile = path.match(
				/^\/api\/admin\/users\/([^/]+)\/subscription-mihomo-profile$/,
			);
			if (userProfile) {
				const id = decodeURIComponent(userProfile[1]);
				const userResponse = await mock.handle(
					new Request(new URL(`/api/admin/users/${id}`, url)),
				);
				if (!userResponse.ok) return userResponse;
				if (method === "PUT") {
					const value = AdminUserMihomoProfileSchema.safeParse(
						await request.json(),
					);
					if (!value.success)
						return errorResponse(
							400,
							"invalid_request",
							"Invalid Mihomo profile.",
						);
					profiles.set(id, value.data);
				}
				if (method === "GET" || method === "PUT")
					return jsonResponse(
						profiles.get(id) ?? {
							mixin_yaml: "",
							extra_proxies_yaml: "",
							extra_proxy_providers_yaml: "",
						},
					);
			}
			const subscription = path.match(/^\/api\/sub\/([^/]+)$/);
			if (subscription && url.searchParams.get("format") === "mihomo") {
				if (method !== "GET")
					return errorResponse(
						405,
						"method_not_allowed",
						"Mihomo subscriptions are read-only in the Demo.",
					);
				const list = await mock.handle(
					new Request(new URL("/api/admin/users", url)),
				);
				const { items } = (await list.json()) as { items: typeof users };
				const user = items.find(
					(item) =>
						item.subscription_token === decodeURIComponent(subscription[1]),
				);
				if (!user)
					return errorResponse(
						404,
						"not_found",
						"Subscription token not found.",
					);
				return renderDemoMihomoSubscription(profiles.get(user.user_id), url);
			}
			if (path === "/api/admin/mesh/status" && method === "GET")
				return jsonResponse(mesh);
			if (path === "/api/admin/mesh/probes" && method === "POST") {
				mesh = { ...mesh, revision: mesh.revision + 1 };
				return jsonResponse({
					accepted_node_ids: mesh.peers.map((peer) => peer.node_id),
					revision: mesh.revision,
				});
			}
			if (path === "/api/admin/mesh/config" && method === "PUT") {
				const body = (await request.json()) as { enabled: boolean };
				mesh = { ...mesh, cluster_mesh_enabled: body.enabled };
				return jsonResponse({ enabled: mesh.cluster_mesh_enabled });
			}
			if (path.startsWith("/api/mihomo/resources/") && method !== "GET")
				return errorResponse(
					405,
					"method_not_allowed",
					"Mihomo resources are read-only in the Demo.",
				);
			if (path.startsWith("/api/mihomo/resources/"))
				return textResponse("# Simulated resource; no upstream fetch.\n");
			return mock.handle(request);
		},
	};
}
