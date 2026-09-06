import { describe, expect, it } from "vitest";

import {
	applyRuntimePolicyGrant,
	parseRuntimePolicyGrant,
} from "./runtimePolicyWorker";

describe("runtime policy worker contract", () => {
	it("accepts only a future HTTPS grant", () => {
		const grant = parseRuntimePolicyGrant(
			{
				policy_id: "rcp-1",
				cluster_id: "cluster-1",
				expires_at: "2099-01-01T00:10:00Z",
				api_origins: ["https://node.example/"],
			},
			0,
		);
		expect(grant).toEqual({
			policyId: "rcp-1",
			clusterId: "cluster-1",
			expiresAt: Date.parse("2099-01-01T00:10:00Z"),
			apiOrigins: ["https://node.example"],
		});
		expect(
			parseRuntimePolicyGrant(
				{
					policy_id: "rcp-1",
					cluster_id: "cluster-1",
					expires_at: "2020-01-01T00:10:00Z",
					api_origins: ["https://node.example"],
				},
				Date.parse("2021-01-01T00:00:00Z"),
			),
		).toBeNull();
	});

	it("rewrites only the document CSP", async () => {
		const response = new Response("shell", {
			headers: {
				"Content-Security-Policy":
					"default-src 'self'; connect-src 'self' https://old.example",
			},
		});
		const rewritten = applyRuntimePolicyGrant(response, {
			policyId: "rcp-1",
			clusterId: "cluster-1",
			expiresAt: Date.now() + 60_000,
			apiOrigins: ["https://node.example"],
		});
		expect(rewritten?.headers.get("Content-Security-Policy")).toBe(
			"default-src 'self'; connect-src 'self' https://node.example",
		);
		expect(await rewritten?.text()).toBe("shell");
	});
});
