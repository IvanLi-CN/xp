import { afterEach, describe, expect, it, vi } from "vitest";

import { fixtureCatalog } from "../fixture-policy/catalog";
import { RuntimePolicySchema, fetchRuntimePolicy } from "./runtimePolicy";

afterEach(() => {
	vi.restoreAllMocks();
	vi.useRealTimers();
});

describe("RuntimePolicySchema", () => {
	it("accepts a short-lived canonical origin policy", () => {
		vi.useFakeTimers({ now: Date.parse("2025-12-01T00:00:00Z") });
		const policy = RuntimePolicySchema.parse({
			policy_id: "rcp-test",
			cluster_id: fixtureCatalog.cluster.fixture84(),
			issued_at: fixtureCatalog.timestamp.releaseCurrent(),
			expires_at: fixtureCatalog.timestamp.releaseHttp(),
			api_origins: ["https://101-xp.ivanli.cc"],
		});
		expect(policy.api_origins).toEqual(["https://101-xp.ivanli.cc"]);
	});

	it("rejects non-url origins", () => {
		vi.useFakeTimers({ now: Date.parse("2025-12-01T00:00:00Z") });
		expect(() =>
			RuntimePolicySchema.parse({
				policy_id: "rcp-test",
				cluster_id: fixtureCatalog.cluster.fixture84(),
				issued_at: fixtureCatalog.timestamp.releaseCurrent(),
				expires_at: fixtureCatalog.timestamp.releaseHttp(),
				api_origins: ["http://insecure.example"],
			}),
		).toThrow();
		expect(() =>
			RuntimePolicySchema.parse({
				policy_id: "rcp-test",
				cluster_id: fixtureCatalog.cluster.fixture84(),
				issued_at: fixtureCatalog.timestamp.baseline(),
				expires_at: fixtureCatalog.timestamp.recent(),
				api_origins: ["not-an-origin"],
			}),
		).toThrow();
	});

	it("rejects an expired grant", () => {
		expect(() =>
			RuntimePolicySchema.parse({
				policy_id: "rcp-expired",
				cluster_id: fixtureCatalog.cluster.fixture95(),
				issued_at: fixtureCatalog.timestamp.baseline(),
				expires_at: fixtureCatalog.timestamp.recent(),
				api_origins: ["https://node.example"],
			}),
		).toThrow();
	});

	it("uses the authenticated runtime-policy route", async () => {
		vi.useFakeTimers({ now: Date.parse("2025-12-01T00:00:00Z") });
		const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
			new Response(
				JSON.stringify({
					policy_id: "rcp-test",
					cluster_id: fixtureCatalog.cluster.fixture84(),
					issued_at: fixtureCatalog.timestamp.releaseCurrent(),
					expires_at: fixtureCatalog.timestamp.releaseHttp(),
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
