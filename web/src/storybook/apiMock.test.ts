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
					cycle_policy_default: "by_user",
					cycle_day_of_month_default: 7,
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

	it("respects grant note tri-state on patch", async () => {
		const mock = createMockApi();
		const listRes = await mock.handle(
			jsonRequest("/api/admin/grants", { method: "GET" }),
		);
		expect(listRes.ok).toBe(true);
		const listData = (await listRes.json()) as {
			items: Array<{
				grant_id: string;
				enabled: boolean;
				quota_limit_bytes: number;
				cycle_policy: string;
				cycle_day_of_month: number | null;
				note: string | null;
			}>;
		};
		const grant = listData.items[0];

		const basePayload = {
			enabled: grant.enabled,
			quota_limit_bytes: grant.quota_limit_bytes,
			cycle_policy: grant.cycle_policy,
			cycle_day_of_month: grant.cycle_day_of_month,
		};

		const keepRes = await mock.handle(
			jsonRequest(`/api/admin/grants/${grant.grant_id}`, {
				method: "PATCH",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify(basePayload),
			}),
		);
		expect(keepRes.ok).toBe(true);
		const keepData = (await keepRes.json()) as { note: string | null };
		expect(keepData.note).toBe(grant.note);

		const clearRes = await mock.handle(
			jsonRequest(`/api/admin/grants/${grant.grant_id}`, {
				method: "PATCH",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({ ...basePayload, note: null }),
			}),
		);
		expect(clearRes.ok).toBe(true);
		const clearData = (await clearRes.json()) as { note: string | null };
		expect(clearData.note).toBeNull();

		const setRes = await mock.handle(
			jsonRequest(`/api/admin/grants/${grant.grant_id}`, {
				method: "PATCH",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({ ...basePayload, note: "Updated note" }),
			}),
		);
		expect(setRes.ok).toBe(true);
		const setData = (await setRes.json()) as { note: string | null };
		expect(setData.note).toBe("Updated note");
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
		const token = usersData.items[0]?.subscription_token ?? "sub-user-1";

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
});
