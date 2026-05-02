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

	it("supports user access hard-cut replace", async () => {
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
			jsonRequest(`/api/admin/users/${userId}/access`, { method: "GET" }),
		);
		expect(listRes.ok).toBe(true);
		const listData = (await listRes.json()) as {
			items: Array<{ user_id: string; endpoint_id: string; node_id: string }>;
			auto_assign_endpoint_kinds: string[];
		};
		expect(listData.items.length).toBeGreaterThan(0);
		expect(listData.items.every((item) => item.user_id === userId)).toBe(true);
		expect(listData.items.every((item) => item.node_id.length > 0)).toBe(true);
		expect(listData.auto_assign_endpoint_kinds).toEqual([
			"vless_reality_vision_tcp",
		]);

		const replaceRes = await mock.handle(
			jsonRequest(`/api/admin/users/${userId}/access`, {
				method: "PUT",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({
					items: [
						{
							endpoint_id: "endpoint-1",
						},
					],
				}),
			}),
		);
		expect(replaceRes.ok).toBe(true);
		const replaced = (await replaceRes.json()) as {
			created: number;
			deleted: number;
			items: Array<{ user_id: string; endpoint_id: string; node_id: string }>;
			auto_assign_endpoint_kinds: string[];
		};
		expect(replaced.created + replaced.deleted).toBeGreaterThanOrEqual(0);
		expect(replaced.items).toHaveLength(1);
		expect(replaced.items[0]?.endpoint_id).toBe("endpoint-1");
		expect(replaced.items[0]?.user_id).toBe(userId);
		expect(replaced.items[0]?.node_id.length).toBeGreaterThan(0);
		expect(replaced.auto_assign_endpoint_kinds).toEqual([
			"vless_reality_vision_tcp",
		]);

		const clearRes = await mock.handle(
			jsonRequest(`/api/admin/users/${userId}/access`, {
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
			auto_assign_endpoint_kinds: string[];
		};
		expect(clearData.deleted).toBeGreaterThanOrEqual(1);
		expect(clearData.items).toHaveLength(0);
		expect(clearData.auto_assign_endpoint_kinds).toEqual([]);

		const detailsRes = await mock.handle(
			jsonRequest(`/api/admin/users/${userId}/access`, {
				method: "GET",
			}),
		);
		expect(detailsRes.ok).toBe(true);
		const detailsData = (await detailsRes.json()) as {
			items: Array<unknown>;
			auto_assign_endpoint_kinds: string[];
		};
		expect(detailsData.items).toHaveLength(0);
		expect(detailsData.auto_assign_endpoint_kinds).toEqual([]);
	});

	it("rejects invalid items in user access hard-cut replace", async () => {
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

		const replaceRes = await mock.handle(
			jsonRequest(`/api/admin/users/${userId}/access`, {
				method: "PUT",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({
					items: [
						{
							endpoint_id: "endpoint-1",
						},
						{ endpoint_id: "" },
					],
				}),
			}),
		);
		expect(replaceRes.status).toBe(400);
		const payload = (await replaceRes.json()) as {
			error: { code: string };
		};
		expect(payload.error.code).toBe("invalid_request");
	});

	it("dedups duplicate endpoint items in user access hard-cut replace", async () => {
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

		const replaceRes = await mock.handle(
			jsonRequest(`/api/admin/users/${userId}/access`, {
				method: "PUT",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({
					items: [
						{
							endpoint_id: "endpoint-1",
						},
						{
							endpoint_id: "endpoint-1",
						},
					],
				}),
			}),
		);
		expect(replaceRes.ok).toBe(true);
		const payload = (await replaceRes.json()) as {
			items: Array<{ endpoint_id: string }>;
			auto_assign_endpoint_kinds: string[];
		};
		expect(
			payload.items.filter((i) => i.endpoint_id === "endpoint-1"),
		).toHaveLength(1);
		expect(payload.auto_assign_endpoint_kinds).toEqual([
			"vless_reality_vision_tcp",
		]);
	});

	it("auto-assigns newly created endpoints by persisted kind intent", async () => {
		const mock = createMockApi();
		const usersRes = await mock.handle(
			jsonRequest("/api/admin/users", { method: "GET" }),
		);
		expect(usersRes.ok).toBe(true);
		const usersData = (await usersRes.json()) as {
			items: Array<{ user_id: string }>;
		};
		const vlessUserId = usersData.items[0]?.user_id ?? "";
		const ssUserId = usersData.items[1]?.user_id ?? "";
		expect(vlessUserId.length).toBeGreaterThan(0);
		expect(ssUserId.length).toBeGreaterThan(0);

		const createEndpointRes = await mock.handle(
			jsonRequest("/api/admin/endpoints", {
				method: "POST",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({
					node_id: "node-1",
					kind: "vless_reality_vision_tcp",
					port: 9443,
					reality: {
						dest: "example.com:443",
						server_names: ["example.com"],
						server_names_source: "manual",
						fingerprint: "chrome",
					},
				}),
			}),
		);
		expect(createEndpointRes.ok).toBe(true);
		const createdEndpoint = (await createEndpointRes.json()) as {
			endpoint_id: string;
		};

		const vlessAccessRes = await mock.handle(
			jsonRequest(`/api/admin/users/${vlessUserId}/access`, { method: "GET" }),
		);
		expect(vlessAccessRes.ok).toBe(true);
		const vlessAccess = (await vlessAccessRes.json()) as {
			items: Array<{ endpoint_id: string }>;
			auto_assign_endpoint_kinds: string[];
		};
		expect(vlessAccess.items.map((item) => item.endpoint_id).sort()).toEqual([
			"endpoint-1",
			createdEndpoint.endpoint_id,
		]);
		expect(vlessAccess.auto_assign_endpoint_kinds).toEqual([
			"vless_reality_vision_tcp",
		]);

		const ssAccessRes = await mock.handle(
			jsonRequest(`/api/admin/users/${ssUserId}/access`, { method: "GET" }),
		);
		expect(ssAccessRes.ok).toBe(true);
		const ssAccess = (await ssAccessRes.json()) as {
			items: Array<{ endpoint_id: string }>;
			auto_assign_endpoint_kinds: string[];
		};
		expect(ssAccess.items.map((item) => item.endpoint_id)).toEqual([
			"endpoint-2",
		]);
		expect(ssAccess.auto_assign_endpoint_kinds).toEqual([
			"ss2022_2022_blake3_aes_128_gcm",
		]);
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

	it("rejects removed admin config patch route", async () => {
		const mock = createMockApi();

		const patchRes = await mock.handle(
			jsonRequest("/api/admin/config", {
				method: "PATCH",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({
					mihomo_delivery_mode: "provider",
				}),
			}),
		);
		expect(patchRes.status).toBe(404);

		const getRes = await mock.handle(
			jsonRequest("/api/admin/config", { method: "GET" }),
		);
		expect(getRes.ok).toBe(true);
		const getData = (await getRes.json()) as Record<string, unknown>;
		expect(getData.mihomo_delivery_mode).toBeUndefined();
	});

	it("supports explicit mihomo provider routes", async () => {
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

		const providerRes = await mock.handle(
			jsonRequest(`/api/sub/${encodeURIComponent(token)}/mihomo/provider`, {
				method: "GET",
				headers: { Accept: "text/plain" },
			}),
		);
		expect(providerRes.ok).toBe(true);
		expect(await providerRes.text()).toContain("xp-system-generated");

		const providerSystemRes = await mock.handle(
			jsonRequest(
				`/api/sub/${encodeURIComponent(token)}/mihomo/provider/system`,
				{
					method: "GET",
					headers: { Accept: "text/plain" },
				},
			),
		);
		expect(providerSystemRes.ok).toBe(true);
		expect(await providerSystemRes.text()).toContain("mock-system");
	});

	it("returns not found for removed mihomo legacy route", async () => {
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

		const legacyRes = await mock.handle(
			jsonRequest(`/api/sub/${encodeURIComponent(token)}/mihomo/legacy`, {
				method: "GET",
				headers: { Accept: "text/plain" },
			}),
		);
		expect(legacyRes.status).toBe(404);
	});

	it("supports mihomo redact preview endpoint", async () => {
		const mock = createMockApi();
		const res = await mock.handle(
			jsonRequest("/api/admin/tools/mihomo/redact", {
				method: "POST",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({
					source_kind: "text",
					source: "server: edge.example.com\npassword: super-secret\n",
					level: "credentials_and_address",
					source_format: "yaml",
				}),
			}),
		);
		expect(res.ok).toBe(true);
		const payload = (await res.json()) as { redacted_text: string };
		expect(payload.redacted_text).toContain("e***.example.com");
		expect(payload.redacted_text).toContain("supe***cret");
	});

	it("rejects private mihomo redact URLs in storybook mock", async () => {
		const mock = createMockApi();
		const res = await mock.handle(
			jsonRequest("/api/admin/tools/mihomo/redact", {
				method: "POST",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({
					source_kind: "url",
					source: "http://127.0.0.1:8080/raw",
					level: "credentials",
					source_format: "auto",
				}),
			}),
		);
		expect(res.status).toBe(400);
		const payload = (await res.json()) as { error: { code: string } };
		expect(payload.error.code).toBe("invalid_request");
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

	it("supports node and user ip usage endpoints", async () => {
		const mock = createMockApi();

		const nodesRes = await mock.handle(
			jsonRequest("/api/admin/nodes", { method: "GET" }),
		);
		expect(nodesRes.ok).toBe(true);
		const nodes = (await nodesRes.json()) as {
			items: Array<{ node_id: string }>;
		};
		const nodeId = nodes.items[0]?.node_id ?? "node-1";

		const nodeUsageRes = await mock.handle(
			jsonRequest(`/api/admin/nodes/${nodeId}/ip-usage?window=7d`, {
				method: "GET",
			}),
		);
		expect(nodeUsageRes.ok).toBe(true);
		const nodeUsage = (await nodeUsageRes.json()) as {
			window: string;
			ips: Array<{ ip: string }>;
		};
		expect(nodeUsage.window).toBe("7d");
		expect(nodeUsage.ips.length).toBeGreaterThan(0);

		const usersRes = await mock.handle(
			jsonRequest("/api/admin/users", { method: "GET" }),
		);
		expect(usersRes.ok).toBe(true);
		const users = (await usersRes.json()) as {
			items: Array<{ user_id: string }>;
		};
		const userId = users.items[0]?.user_id ?? "";
		expect(userId.length).toBeGreaterThan(0);

		const userUsageRes = await mock.handle(
			jsonRequest(`/api/admin/users/${userId}/ip-usage?window=24h`, {
				method: "GET",
			}),
		);
		expect(userUsageRes.ok).toBe(true);
		const userUsage = (await userUsageRes.json()) as {
			groups: Array<{ node: { node_id: string } }>;
		};
		expect(userUsage.groups.length).toBeGreaterThan(0);
	});

	it("supports node egress probe refresh", async () => {
		const mock = createMockApi({
			data: {
				nodes: [
					{
						node_id: "node-egress-1",
						node_name: "node-egress-1",
						access_host: "node-egress-1.example.invalid",
						api_base_url: "https://node-egress-1.example.invalid",
						quota_limit_bytes: 0,
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: 0,
						},
						egress_probe: {
							public_ipv4: "203.0.113.8",
							public_ipv6: "2001:db8::8",
							selected_public_ip: "203.0.113.8",
							country_code: "TW",
							geo_region: "Taiwan",
							geo_city: "Taipei",
							geo_operator: "HiNet",
							subscription_region: "taiwan",
							checked_at: "2026-04-24T00:00:00Z",
							last_success_at: "2026-04-24T00:00:00Z",
							stale: false,
							error_summary: null,
						},
					},
				],
			},
		});

		const nodesRes = await mock.handle(
			jsonRequest("/api/admin/nodes", { method: "GET" }),
		);
		expect(nodesRes.ok).toBe(true);
		const nodes = (await nodesRes.json()) as {
			items: Array<{
				node_id: string;
				egress_probe?: { subscription_region: string };
			}>;
		};
		const nodeId = nodes.items[0]?.node_id ?? "node-egress-1";

		const refreshRes = await mock.handle(
			jsonRequest(`/api/admin/nodes/${nodeId}/egress-probe/refresh`, {
				method: "POST",
			}),
		);
		expect(refreshRes.ok).toBe(true);
		const payload = (await refreshRes.json()) as {
			node_id: string;
			accepted: boolean;
			egress_probe?: { subscription_region: string };
		};
		expect(payload.node_id).toBe(nodeId);
		expect(payload.accepted).toBe(true);
		expect(payload.egress_probe?.subscription_region).toBeTruthy();
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
