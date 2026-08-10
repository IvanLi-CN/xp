import { describe, expect, it } from "vitest";
import { fixtureCatalog } from "../fixture-policy/catalog";

import { createMockApi } from "../../.storybook/mocks/apiMock";

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
	});
});
