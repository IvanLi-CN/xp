import { describe, expect, it } from "vitest";

import { createMockApi } from "../../.storybook/mocks/apiMock";

const baseUrl = "http://localhost";

function jsonRequest(path: string, init?: RequestInit) {
	return new Request(`${baseUrl}${path}`, init);
}

describe("storybook api mock", () => {
	it("supports users CRUD", async () => {
		const mock = createMockApi();

		const listRes = await mock.handle(
			jsonRequest("/api/admin/users", { method: "GET" }),
		);
		expect(listRes.ok).toBe(true);
		const listData = (await listRes.json()) as {
			items: Array<{ user_id: string }>;
		};
		const initialCount = listData.items.length;

		const createRes = await mock.handle(
			jsonRequest("/api/admin/users", {
				method: "POST",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({
					display_name: "New User",
					quota_reset: {
						policy: "monthly",
						day_of_month: 7,
						tz_offset_minutes: 480,
					},
				}),
			}),
		);
		expect(createRes.ok).toBe(true);
		const created = (await createRes.json()) as { user_id: string };
		expect(created.user_id).toContain("user-mock-");

		const patchRes = await mock.handle(
			jsonRequest(`/api/admin/users/${created.user_id}`, {
				method: "PATCH",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({
					display_name: "Renamed User",
				}),
			}),
		);
		expect(patchRes.ok).toBe(true);
		const patched = (await patchRes.json()) as { display_name: string };
		expect(patched.display_name).toBe("Renamed User");

		const deleteRes = await mock.handle(
			jsonRequest(`/api/admin/users/${created.user_id}`, {
				method: "DELETE",
			}),
		);
		expect(deleteRes.status).toBe(204);

		const listResAfter = await mock.handle(
			jsonRequest("/api/admin/users", { method: "GET" }),
		);
		expect(listResAfter.ok).toBe(true);
		const listDataAfter = (await listResAfter.json()) as {
			items: Array<{ user_id: string }>;
		};
		expect(listDataAfter.items.length).toBe(initialCount);
	});

	it("supports user grants hard-cut replace", async () => {
		const mock = createMockApi();

		const usersRes = await mock.handle(
			jsonRequest("/api/admin/users", { method: "GET" }),
		);
		expect(usersRes.ok).toBe(true);
		const usersData = (await usersRes.json()) as {
			items: Array<{ user_id: string }>;
		};
		const userId = usersData.items[0]?.user_id ?? "";
		expect(userId.length).toBeGreaterThan(0);

		const listRes = await mock.handle(
			jsonRequest(`/api/admin/users/${userId}/grants`, { method: "GET" }),
		);
		expect(listRes.ok).toBe(true);
		const listData = (await listRes.json()) as {
			items: Array<{ endpoint_id: string; enabled: boolean }>;
		};
		expect(listData.items.length).toBeGreaterThan(0);
		expect(listData.items.every((item) => item.enabled)).toBe(true);

		const replaceRes = await mock.handle(
			jsonRequest(`/api/admin/users/${userId}/grants`, {
				method: "PUT",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({
					items: [
						{
							endpoint_id: "endpoint-1",
							enabled: true,
							quota_limit_bytes: 123,
							note: "hello",
						},
					],
				}),
			}),
		);
		expect(replaceRes.ok).toBe(true);
		const replaced = (await replaceRes.json()) as {
			created: number;
			updated: number;
			deleted: number;
			items: Array<{ endpoint_id: string; note: string | null }>;
		};
		expect(replaced.created + replaced.updated).toBeGreaterThan(0);
		expect(replaced.items).toHaveLength(1);
		expect(replaced.items[0]?.endpoint_id).toBe("endpoint-1");
		expect(replaced.items[0]?.note).toBe("hello");

		const clearRes = await mock.handle(
			jsonRequest(`/api/admin/users/${userId}/grants`, {
				method: "PUT",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({
					items: [],
				}),
			}),
		);
		expect(clearRes.ok).toBe(true);
		const clearData = (await clearRes.json()) as {
			deleted: number;
			items: Array<unknown>;
		};
		expect(clearData.deleted).toBeGreaterThanOrEqual(1);
		expect(clearData.items).toHaveLength(0);

		const detailsRes = await mock.handle(
			jsonRequest(`/api/admin/users/${userId}/grants`, {
				method: "GET",
			}),
		);
		expect(detailsRes.ok).toBe(true);
		const detailsData = (await detailsRes.json()) as {
			items: Array<unknown>;
		};
		expect(detailsData.items).toHaveLength(0);
	});

	it("returns not_found for retired grant-groups routes", async () => {
		const mock = createMockApi();
		const res = await mock.handle(
			jsonRequest("/api/admin/grant-groups", { method: "GET" }),
		);
		expect(res.status).toBe(404);
		const payload = (await res.json()) as {
			error: { code: string };
		};
		expect(payload.error.code).toBe("not_found");
	});

	it("returns subscription text response", async () => {
		const mock = createMockApi();
		const usersRes = await mock.handle(
			jsonRequest("/api/admin/users", { method: "GET" }),
		);
		expect(usersRes.ok).toBe(true);
		const usersData = (await usersRes.json()) as {
			items: Array<{ subscription_token: string }>;
		};
		const token = usersData.items[0]?.subscription_token ?? "";
		expect(token.length).toBeGreaterThan(0);

		const subRes = await mock.handle(
			jsonRequest(`/api/sub/${encodeURIComponent(token)}`, {
				method: "GET",
				headers: { Accept: "text/plain" },
			}),
		);
		expect(subRes.ok).toBe(true);
		expect(subRes.headers.get("content-type")).toContain("text/plain");
		const text = await subRes.text();
		expect(text).toContain(token);
	});

	it("supports user node quotas", async () => {
		const mock = createMockApi();

		const listUsers = await mock.handle(
			jsonRequest("/api/admin/users", { method: "GET" }),
		);
		expect(listUsers.ok).toBe(true);
		const usersData = (await listUsers.json()) as {
			items: Array<{ user_id: string }>;
		};
		const userId = usersData.items[0]?.user_id ?? "";
		expect(userId.length).toBeGreaterThan(0);

		const listNodes = await mock.handle(
			jsonRequest("/api/admin/nodes", { method: "GET" }),
		);
		expect(listNodes.ok).toBe(true);
		const nodesData = (await listNodes.json()) as {
			items: Array<{ node_id: string }>;
		};
		const nodeId = nodesData.items[0]?.node_id ?? "node-1";

		const putRes = await mock.handle(
			jsonRequest(`/api/admin/users/${userId}/node-quotas/${nodeId}`, {
				method: "PUT",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({
					quota_limit_bytes: 10 * 2 ** 30,
					quota_reset_source: "node",
				}),
			}),
		);
		expect(putRes.ok).toBe(true);

		const listRes = await mock.handle(
			jsonRequest(`/api/admin/users/${userId}/node-quotas`, { method: "GET" }),
		);
		expect(listRes.ok).toBe(true);
		const listData = (await listRes.json()) as {
			items: Array<{
				user_id: string;
				node_id: string;
				quota_limit_bytes: number;
				quota_reset_source: string;
			}>;
		};
		expect(
			listData.items.some(
				(item) =>
					item.user_id === userId &&
					item.node_id === nodeId &&
					item.quota_limit_bytes === 10 * 2 ** 30 &&
					item.quota_reset_source === "node",
			),
		).toBe(true);
	});
});
