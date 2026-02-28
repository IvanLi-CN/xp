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

	it("supports user access GET/PUT hard-cut", async () => {
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

		const listNodes = await mock.handle(
			jsonRequest("/api/admin/nodes", { method: "GET" }),
		);
		expect(listNodes.ok).toBe(true);
		const nodesData = (await listNodes.json()) as {
			items: Array<{ node_id: string }>;
		};
		const nodeId = nodesData.items[0]?.node_id ?? "";
		expect(nodeId.length).toBeGreaterThan(0);

		const listEndpoints = await mock.handle(
			jsonRequest("/api/admin/endpoints", { method: "GET" }),
		);
		expect(listEndpoints.ok).toBe(true);
		const endpointsData = (await listEndpoints.json()) as {
			items: Array<{ endpoint_id: string; node_id: string }>;
		};
		const endpointOnNode =
			endpointsData.items.find((item) => item.node_id === nodeId) ??
			endpointsData.items[0];
		expect(endpointOnNode).toBeTruthy();
		if (!endpointOnNode) {
			throw new Error("expected endpoint for access test");
		}

		const getRes = await mock.handle(
			jsonRequest(`/api/admin/users/${userId}/access`, { method: "GET" }),
		);
		expect(getRes.ok).toBe(true);
		const beforeData = (await getRes.json()) as {
			items: Array<{ membership: { user_id: string } }>;
		};
		const initialCount = beforeData.items.length;

		const putRes = await mock.handle(
			jsonRequest(`/api/admin/users/${userId}/access`, {
				method: "PUT",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({
					items: [{ endpoint_id: endpointOnNode.endpoint_id, note: "hello" }],
				}),
			}),
		);
		expect(putRes.ok).toBe(true);
		const putData = (await putRes.json()) as {
			items: Array<{
				membership: { user_id: string; endpoint_id: string };
				grant: { enabled: boolean; note: string | null };
			}>;
		};
		expect(putData.items).toHaveLength(1);
		expect(putData.items[0]?.membership.user_id).toBe(userId);
		expect(putData.items[0]?.membership.endpoint_id).toBe(
			endpointOnNode.endpoint_id,
		);
		expect(putData.items[0]?.grant.enabled).toBe(true);
		expect(putData.items[0]?.grant.note).toBe("hello");

		const getAfterRes = await mock.handle(
			jsonRequest(`/api/admin/users/${userId}/access`, { method: "GET" }),
		);
		expect(getAfterRes.ok).toBe(true);
		const afterData = (await getAfterRes.json()) as typeof putData;
		expect(afterData.items).toHaveLength(1);

		const clearRes = await mock.handle(
			jsonRequest(`/api/admin/users/${userId}/access`, {
				method: "PUT",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({ items: [] }),
			}),
		);
		expect(clearRes.ok).toBe(true);
		const clearData = (await clearRes.json()) as { items: unknown[] };
		expect(clearData.items).toEqual([]);

		expect(initialCount).toBeGreaterThanOrEqual(0);
	});

	it("returns 404 for removed grant-group routes", async () => {
		const mock = createMockApi();

		const listRes = await mock.handle(
			jsonRequest("/api/admin/grant-groups", { method: "GET" }),
		);
		expect(listRes.ok).toBe(false);
		expect(listRes.status).toBe(404);
		const getRes = await mock.handle(
			jsonRequest("/api/admin/grant-groups/test-group", { method: "GET" }),
		);
		expect(getRes.status).toBe(404);
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

	it("supports user node weights", async () => {
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

		const listRes = await mock.handle(
			jsonRequest(`/api/admin/users/${userId}/node-weights`, { method: "GET" }),
		);
		expect(listRes.ok).toBe(true);
		const listData = (await listRes.json()) as {
			items: Array<{
				node_id: string;
				weight: number;
			}>;
		};

		const putRes = await mock.handle(
			jsonRequest(`/api/admin/users/${userId}/node-weights/${nodeId}`, {
				method: "PUT",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({
					weight: 123,
				}),
			}),
		);
		expect(putRes.ok).toBe(true);

		const listResAfter = await mock.handle(
			jsonRequest(`/api/admin/users/${userId}/node-weights`, { method: "GET" }),
		);
		expect(listResAfter.ok).toBe(true);
		const listDataAfter = (await listResAfter.json()) as typeof listData;
		expect(
			listDataAfter.items.some(
				(item) => item.node_id === nodeId && item.weight === 123,
			),
		).toBe(true);
	});

	it("supports quota policy node weight rows", async () => {
		const mock = createMockApi();

		const listNodes = await mock.handle(
			jsonRequest("/api/admin/nodes", { method: "GET" }),
		);
		expect(listNodes.ok).toBe(true);
		const nodesData = (await listNodes.json()) as {
			items: Array<{ node_id: string }>;
		};
		const nodeId = nodesData.items[0]?.node_id ?? "node-1";

		const rowsRes = await mock.handle(
			jsonRequest(`/api/admin/quota-policy/nodes/${nodeId}/weight-rows`, {
				method: "GET",
			}),
		);
		expect(rowsRes.ok).toBe(true);
		const rowsData = (await rowsRes.json()) as {
			items: Array<{
				user_id: string;
				editor_weight: number;
				source: "explicit" | "implicit_zero";
				endpoint_ids: string[];
			}>;
		};
		expect(rowsData.items.length).toBeGreaterThan(0);
		expect(rowsData.items[0]?.endpoint_ids.length).toBeGreaterThan(0);

		const row = rowsData.items[0];
		if (!row) {
			throw new Error("expected at least one row");
		}
		await mock.handle(
			jsonRequest(`/api/admin/users/${row.user_id}/node-weights/${nodeId}`, {
				method: "PUT",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({ weight: 321 }),
			}),
		);

		const rowsAfterRes = await mock.handle(
			jsonRequest(`/api/admin/quota-policy/nodes/${nodeId}/weight-rows`, {
				method: "GET",
			}),
		);
		expect(rowsAfterRes.ok).toBe(true);
		const rowsAfter = (await rowsAfterRes.json()) as typeof rowsData;
		expect(rowsAfter.items.some((item) => item.editor_weight === 321)).toBe(
			true,
		);
	});

	it("supports quota policy global weight rows and node inherit policy", async () => {
		const mock = createMockApi();

		const globalRowsRes = await mock.handle(
			jsonRequest("/api/admin/quota-policy/global-weight-rows", {
				method: "GET",
			}),
		);
		expect(globalRowsRes.ok).toBe(true);
		const globalRows = (await globalRowsRes.json()) as {
			items: Array<{ user_id: string; editor_weight: number }>;
		};
		const firstUserId = globalRows.items[0]?.user_id;
		expect(firstUserId).toBeTruthy();
		if (!firstUserId) {
			throw new Error("expected at least one global weight row");
		}

		const putGlobalRes = await mock.handle(
			jsonRequest(`/api/admin/quota-policy/global-weight-rows/${firstUserId}`, {
				method: "PUT",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({ weight: 777 }),
			}),
		);
		expect(putGlobalRes.ok).toBe(true);

		const globalRowsAfterRes = await mock.handle(
			jsonRequest("/api/admin/quota-policy/global-weight-rows", {
				method: "GET",
			}),
		);
		expect(globalRowsAfterRes.ok).toBe(true);
		const globalRowsAfter =
			(await globalRowsAfterRes.json()) as typeof globalRows;
		expect(
			globalRowsAfter.items.some(
				(item) => item.user_id === firstUserId && item.editor_weight === 777,
			),
		).toBe(true);

		const listNodes = await mock.handle(
			jsonRequest("/api/admin/nodes", { method: "GET" }),
		);
		expect(listNodes.ok).toBe(true);
		const nodesData = (await listNodes.json()) as {
			items: Array<{ node_id: string }>;
		};
		const nodeId = nodesData.items[0]?.node_id ?? "node-1";

		const policyRes = await mock.handle(
			jsonRequest(`/api/admin/quota-policy/nodes/${nodeId}/policy`, {
				method: "GET",
			}),
		);
		expect(policyRes.ok).toBe(true);
		const policy = (await policyRes.json()) as { inherit_global: boolean };
		expect(typeof policy.inherit_global).toBe("boolean");

		const putPolicyRes = await mock.handle(
			jsonRequest(`/api/admin/quota-policy/nodes/${nodeId}/policy`, {
				method: "PUT",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({ inherit_global: false }),
			}),
		);
		expect(putPolicyRes.ok).toBe(true);

		const policyAfterRes = await mock.handle(
			jsonRequest(`/api/admin/quota-policy/nodes/${nodeId}/policy`, {
				method: "GET",
			}),
		);
		expect(policyAfterRes.ok).toBe(true);
		const policyAfter = (await policyAfterRes.json()) as {
			inherit_global: boolean;
		};
		expect(policyAfter.inherit_global).toBe(false);
	});
});
