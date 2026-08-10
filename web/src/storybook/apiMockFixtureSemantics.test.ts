import { describe, expect, it } from "vitest";
import { fixtureCatalog } from "../fixture-policy/catalog";

import { createMockApi } from "../../.storybook/mocks/apiMock";
import { handleAdminConfigAndEndpointRoutes } from "../../tests/e2e/adminEndpointRouteMocks";
import { normalizeFixtureEndpoint } from "../../tests/e2e/fixtureStateSanitizers";

const baseUrl = "http://localhost";

function jsonRequest(path: string, init?: RequestInit) {
	return new Request(`${baseUrl}${path}`, init);
}

describe("storybook fixture semantics", () => {
	it("preserves approved user patch settings", async () => {
		const mock = createMockApi();
		const usersResponse = await mock.handle(
			jsonRequest("/api/admin/users", { method: "GET" }),
		);
		const users = (await usersResponse.json()) as {
			items: Array<{ user_id: string }>;
		};
		const userId = users.items[0]?.user_id;
		if (!userId) throw new Error("expected a fixture user");

		const response = await mock.handle(
			jsonRequest(`/api/admin/users/${userId}`, {
				method: "PATCH",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({
					priority_tier: fixtureCatalog.user.priorityTierPrimary(),
					quota_reset: fixtureCatalog.quota.resetUserUnlimited(),
				}),
			}),
		);
		expect(response.ok).toBe(true);
		expect(await response.json()).toMatchObject({
			priority_tier: fixtureCatalog.user.priorityTierPrimary(),
			quota_reset: fixtureCatalog.quota.resetUserUnlimited(),
		});
	});

	it("preserves approved endpoint metadata settings", async () => {
		const mock = createMockApi();
		const response = await mock.handle(
			jsonRequest("/api/admin/endpoints", {
				method: "POST",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({
					node_id: fixtureCatalog.nodeId.fixture32(),
					kind: fixtureCatalog.endpoint.vlessKind(),
					port: fixtureCatalog.endpoint.port9443(),
					canary_upstream: fixtureCatalog.canaryUpstream.httpLoopback(),
					accepted_authorities: fixtureCatalog.authority.edgeExamplePort443(),
				}),
			}),
		);
		expect(response.ok).toBe(true);
		expect(await response.json()).toMatchObject({
			meta: {
				canary_upstream: fixtureCatalog.canaryUpstream.httpLoopback(),
				accepted_authorities: fixtureCatalog.authority.edgeExamplePort443(),
			},
		});
	});

	it("preserves approved endpoint metadata patches and authority clearing", async () => {
		const mock = createMockApi();
		const endpointsResponse = await mock.handle(
			jsonRequest("/api/admin/endpoints", { method: "GET" }),
		);
		const endpoints = (await endpointsResponse.json()) as {
			items: Array<{ endpoint_id: string; kind: string }>;
		};
		const endpointId = endpoints.items.find(
			(endpoint) => endpoint.kind === fixtureCatalog.endpoint.vlessKind(),
		)?.endpoint_id;
		if (!endpointId) throw new Error("expected a VLESS fixture endpoint");

		const updateResponse = await mock.handle(
			jsonRequest(`/api/admin/endpoints/${endpointId}`, {
				method: "PATCH",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({
					port: fixtureCatalog.endpoint.port9443(),
					canary_upstream: fixtureCatalog.canaryUpstream.httpLoopback(),
					accepted_authorities: fixtureCatalog.authority.edgeExamplePort443(),
				}),
			}),
		);
		expect(updateResponse.ok).toBe(true);
		expect(await updateResponse.json()).toMatchObject({
			port: fixtureCatalog.endpoint.port9443(),
			meta: {
				canary_upstream: fixtureCatalog.canaryUpstream.httpLoopback(),
				accepted_authorities: fixtureCatalog.authority.edgeExamplePort443(),
			},
		});

		const clearResponse = await mock.handle(
			jsonRequest(`/api/admin/endpoints/${endpointId}`, {
				method: "PATCH",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({
					accepted_authorities: fixtureCatalog.authority.empty(),
				}),
			}),
		);
		expect(clearResponse.ok).toBe(true);
		const clearedEndpoint = (await clearResponse.json()) as {
			meta: Record<string, unknown>;
		};
		expect(clearedEndpoint.meta).not.toHaveProperty("accepted_authorities");

		const shadowsocksEndpointId = endpoints.items.find(
			(endpoint) => endpoint.kind === fixtureCatalog.endpoint.ssKind(),
		)?.endpoint_id;
		if (!shadowsocksEndpointId) {
			throw new Error("expected a Shadowsocks fixture endpoint");
		}
		const smuxResponse = await mock.handle(
			jsonRequest(`/api/admin/endpoints/${shadowsocksEndpointId}`, {
				method: "PATCH",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({
					mihomo_smux: {
						enabled: false,
						max_connections: 4,
						min_streams: 4,
						only_tcp: true,
					},
				}),
			}),
		);
		expect(smuxResponse.ok).toBe(true);
		expect(await smuxResponse.json()).toMatchObject({
			meta: {
				mihomo_smux: {
					enabled: false,
					max_connections: 4,
					min_streams: 4,
					only_tcp: true,
				},
			},
		});
	});

	it("preserves approved quota status values", async () => {
		const mock = createMockApi({
			data: {
				nodeQuotas: [
					{
						user_id: fixtureCatalog.identifier.userPrimary(),
						node_id: fixtureCatalog.identifier.nodePrimary(),
						quota_limit_bytes: fixtureCatalog.quota.tenGiB(),
						quota_reset_source: fixtureCatalog.quota.resetSource(),
					},
					{
						user_id: fixtureCatalog.identifier.userPrimary(),
						node_id: fixtureCatalog.identifier.nodeSecondary(),
						quota_limit_bytes: fixtureCatalog.quota.fiveGiB(),
						quota_reset_source: fixtureCatalog.quota.resetSourceNode(),
					},
				],
			},
		});
		const response = await mock.handle(
			jsonRequest(
				`/api/admin/users/${fixtureCatalog.identifier.userPrimary()}/node-quotas/status`,
				{ method: "GET" },
			),
		);
		expect(response.ok).toBe(true);
		const payload = (await response.json()) as {
			items: Array<{
				node_id: string;
				quota_limit_bytes: number;
				remaining_bytes: number;
				quota_reset_source: string;
			}>;
		};
		expect(payload.items).toMatchObject([
			{
				node_id: fixtureCatalog.identifier.nodePrimary(),
				quota_limit_bytes: fixtureCatalog.quota.tenGiB(),
				remaining_bytes: fixtureCatalog.quota.tenGiB(),
				quota_reset_source: fixtureCatalog.quota.resetSource(),
			},
			{
				node_id: fixtureCatalog.identifier.nodeSecondary(),
				quota_limit_bytes: fixtureCatalog.quota.fiveGiB(),
				remaining_bytes: fixtureCatalog.quota.fiveGiB(),
				quota_reset_source: fixtureCatalog.quota.resetSourceNode(),
			},
		]);
		const summariesResponse = await mock.handle(
			jsonRequest("/api/admin/users/quota-summaries", { method: "GET" }),
		);
		expect(summariesResponse.ok).toBe(true);
		expect(await summariesResponse.json()).toMatchObject({
			items: [
				{
					user_id: fixtureCatalog.identifier.userPrimary(),
					quota_limit_kind: "fixed",
					quota_limit_bytes: fixtureCatalog.quota.fifteenGiB(),
					remaining_bytes: fixtureCatalog.quota.fifteenGiB(),
				},
			],
		});
	});

	it("keeps unlimited quotas unlimited in status and summaries", async () => {
		const mock = createMockApi({
			data: {
				nodeQuotas: [
					{
						user_id: fixtureCatalog.identifier.userPrimary(),
						node_id: fixtureCatalog.identifier.nodePrimary(),
						quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
						quota_reset_source: fixtureCatalog.quota.resetSource(),
					},
				],
			},
		});
		const statusResponse = await mock.handle(
			jsonRequest(
				`/api/admin/users/${fixtureCatalog.identifier.userPrimary()}/node-quotas/status`,
				{ method: "GET" },
			),
		);
		expect(statusResponse.ok).toBe(true);
		expect(await statusResponse.json()).toMatchObject({
			items: [
				{
					quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
					remaining_bytes: fixtureCatalog.quota.usedBytes(),
					cycle_end_at: null,
				},
			],
		});
		const summariesResponse = await mock.handle(
			jsonRequest("/api/admin/users/quota-summaries", { method: "GET" }),
		);
		expect(summariesResponse.ok).toBe(true);
		expect(await summariesResponse.json()).toMatchObject({
			items: [
				{
					user_id: fixtureCatalog.identifier.userPrimary(),
					quota_limit_kind: "unlimited",
					quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
					remaining_bytes: fixtureCatalog.quota.usedBytes(),
				},
			],
		});
	});

	it("retains approved endpoint seed ports and reality configuration", () => {
		const endpoint = normalizeFixtureEndpoint(
			{
				endpoint_id: fixtureCatalog.identifier.endpointSecondary(),
				node_id: fixtureCatalog.identifier.nodeSecondary(),
				tag: fixtureCatalog.identifier.endpointTagSecondary(),
				kind: fixtureCatalog.endpoint.vlessKind(),
				port: fixtureCatalog.endpoint.port53844(),
				meta: { reality: fixtureCatalog.endpoint.realitySecondary() },
			},
			0,
		);
		expect(endpoint).toMatchObject({
			port: fixtureCatalog.endpoint.port53844(),
			meta: { reality: fixtureCatalog.endpoint.realitySecondary() },
		});
	});

	it("lets E2E endpoint mocks clear approved authorities with an empty list", () => {
		const endpoint = {
			endpoint_id: fixtureCatalog.identifier.endpointPrimary(),
			node_id: fixtureCatalog.identifier.nodePrimary(),
			tag: fixtureCatalog.identifier.endpointTagPrimary(),
			kind: fixtureCatalog.endpoint.vlessKind(),
			port: fixtureCatalog.endpoint.port443(),
			meta: {
				accepted_authorities: fixtureCatalog.authority.edgeExamplePort443(),
			},
		};
		let responseBody = "";
		const handled = handleAdminConfigAndEndpointRoutes({
			path: `/api/admin/endpoints/${fixtureCatalog.identifier.endpointPrimary()}`,
			method: "PATCH",
			route: {
				fulfill: async (response: { body?: string }) => {
					responseBody = response.body ?? "";
				},
			} as never,
			request: {
				postData: () =>
					JSON.stringify({
						accepted_authorities: fixtureCatalog.authority.empty(),
					}),
			},
			state: {
				nodes: [],
				endpoints: [endpoint],
				nextEndpointId: fixtureCatalog.identifier.createEndpointIdFactory(),
				nextEndpointTag: fixtureCatalog.identifier.createEndpointTagFactory(),
			},
		});
		expect(handled).toBe(true);
		expect(endpoint.meta.accepted_authorities).toBeUndefined();
		expect(JSON.parse(responseBody)).not.toHaveProperty(
			"meta.accepted_authorities",
		);
	});
});
