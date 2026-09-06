import { afterEach, describe, expect, it, vi } from "vitest";

import { RuntimePolicySchema, fetchRuntimePolicy } from "./runtimePolicy";

afterEach(() => vi.restoreAllMocks());

describe("RuntimePolicySchema", () => {
	it("accepts a short-lived canonical origin policy", () => {
		const policy = RuntimePolicySchema.parse({
			policy_id: "rcp-test",
			cluster_id: "cluster-1",
			issued_at: "2099-09-06T00:00:00Z",
			expires_at: "2099-09-06T00:10:00Z",
			api_origins: ["https://101-xp.ivanli.cc"],
		});
		expect(policy.api_origins).toEqual(["https://101-xp.ivanli.cc"]);
	});

	it("rejects non-url origins", () => {
		expect(() =>
			RuntimePolicySchema.parse({
				policy_id: "rcp-test",
				cluster_id: "cluster-1",
				issued_at: "2099-09-06T00:00:00Z",
				expires_at: "2099-09-06T00:10:00Z",
				api_origins: ["http://insecure.example"],
			}),
		).toThrow();
		expect(() =>
			RuntimePolicySchema.parse({
				policy_id: "rcp-test",
				cluster_id: "cluster-1",
				issued_at: "2026-09-06T00:00:00Z",
				expires_at: "2026-09-06T00:10:00Z",
				api_origins: ["not-an-origin"],
			}),
		).toThrow();
	});

	it("rejects an expired grant", () => {
		expect(() =>
			RuntimePolicySchema.parse({
				policy_id: "rcp-expired",
				cluster_id: "cluster-a",
				issued_at: "2020-01-01T00:00:00Z",
				expires_at: "2020-01-01T00:10:00Z",
				api_origins: ["https://node.example"],
			}),
		).toThrow();
	});

	it("uses the authenticated runtime-policy route", async () => {
		const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
			new Response(
				JSON.stringify({
					policy_id: "rcp-test",
					cluster_id: "cluster-1",
					issued_at: "2099-09-06T00:00:00Z",
					expires_at: "2099-09-06T00:10:00Z",
					api_origins: ["https://node.example"],
				}),
			),
		);

		await fetchRuntimePolicy("secret-token");
		expect(fetchMock).toHaveBeenCalledWith(
			"/api/admin/console/runtime-policy",
			expect.objectContaining({
				method: "GET",
				headers: expect.objectContaining({
					Authorization: "Bearer secret-token",
				}),
			}),
		);
	});
});
