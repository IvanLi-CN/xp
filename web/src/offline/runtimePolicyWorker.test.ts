import { describe, expect, it } from "vitest";

import { fixtureCatalog } from "../fixture-policy/catalog";
import {
	applyRuntimePolicyGrant,
	parseRuntimePolicyGrant,
} from "./runtimePolicyWorker";

describe("runtime policy worker contract", () => {
	it("accepts only a future HTTPS grant", () => {
		const grant = parseRuntimePolicyGrant(
			{
				policy_id: "rcp-1",
				cluster_id: fixtureCatalog.cluster.fixture84(),
				expires_at: fixtureCatalog.timestamp.releaseHttp(),
				api_origins: ["https://node.example/"],
			},
			0,
		);
		expect(grant).toEqual({
			policyId: "rcp-1",
			clusterId: fixtureCatalog.cluster.fixture84(),
			expiresAt: Date.parse(fixtureCatalog.timestamp.releaseHttp()),
			apiOrigins: ["https://node.example"],
		});
		expect(
			parseRuntimePolicyGrant(
				{
					policy_id: "rcp-1",
					cluster_id: fixtureCatalog.cluster.fixture84(),
					expires_at: fixtureCatalog.timestamp.earlier(),
					api_origins: ["https://node.example"],
				},
				Date.parse(fixtureCatalog.timestamp.baseline()),
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
			clusterId: fixtureCatalog.cluster.fixture84(),
			expiresAt: Date.now() + 60_000,
			apiOrigins: ["https://node.example"],
		});
		expect(rewritten?.headers.get("Content-Security-Policy")).toBe(
			"default-src 'self'; connect-src 'self' https://node.example",
		);
		expect(await rewritten?.text()).toBe("shell");
	});
});
