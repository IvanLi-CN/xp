import { describe, expect, it, vi } from "vitest";

import { fixtureCatalog } from "@/fixture-policy/catalog";
import { createBackendProber } from "./probeBackend";

describe("candidate probe authorization boundaries", () => {
	it("rejects a malformed capabilities response before sending a token", async () => {
		const requests: Request[] = [];
		const origin = fixtureCatalog.url.nodeA();
		const clusterId = fixtureCatalog.cluster.fixture84();
		const prober = createBackendProber({
			directFetch: vi.fn(async (input, init) => {
				const request = new Request(input, init);
				requests.push(request);
				const path = new URL(request.url).pathname;
				return Response.json(
					path === "/api/health"
						? { status: "ok" }
						: path === "/api/cluster/info"
							? { cluster_id: fixtureCatalog.cluster.fixture84() }
							: { release_tag: "v3.22.0" },
				);
			}) as typeof window.fetch,
			canonicalBackendOrigin: (value) => value,
			getState: () => ({
				clusterId,
				primaryOrigin: origin,
				candidates: [],
				generation: 0,
				state: "ready",
				pendingMutations: 0,
				unknownMutationCount: 0,
				lastSwitchTimedOut: false,
			}),
			getPageOrigin: () => origin,
			allowedOrigins: () => [origin],
			isPolicyExpired: () => false,
			isStaticWebConsole: () => false,
			getSessionEpoch: () => 0,
		});
		await expect(
			prober.verifyBackendCandidate({
				origin,
				clusterId,
				adminToken: "test-token",
			}),
		).rejects.toThrow();
		expect(requests).toHaveLength(3);
		expect(
			requests.every((request) => !request.headers.has("Authorization")),
		).toBe(true);
	});

	it("does not send a token when the runtime policy expires during public probing", async () => {
		const requests: Request[] = [];
		const origin = fixtureCatalog.url.nodeA();
		const clusterId = fixtureCatalog.cluster.fixture84();
		let expired = false;
		const prober = createBackendProber({
			directFetch: vi.fn(async (input, init) => {
				const request = new Request(input, init);
				requests.push(request);
				const path = new URL(request.url).pathname;
				if (path === "/api/capabilities") expired = true;
				return Response.json(
					path === "/api/health"
						? { status: "ok" }
						: path === "/api/cluster/info"
							? { cluster_id: fixtureCatalog.cluster.fixture84() }
							: { release_tag: "v3.22.0", capabilities: ["admin.nodes"] },
				);
			}) as typeof window.fetch,
			canonicalBackendOrigin: (value) => value,
			getState: () => ({
				clusterId,
				primaryOrigin: origin,
				candidates: [],
				generation: 0,
				state: "ready",
				pendingMutations: 0,
				unknownMutationCount: 0,
				lastSwitchTimedOut: false,
			}),
			getPageOrigin: () => origin,
			allowedOrigins: () => [origin],
			isPolicyExpired: () => expired,
			isStaticWebConsole: () => true,
			getSessionEpoch: () => 0,
		});
		await expect(
			prober.verifyBackendCandidate({
				origin,
				clusterId,
				adminToken: "test-token",
			}),
		).rejects.toThrow("runtime policy");
		expect(requests).toHaveLength(3);
		expect(
			requests.every((request) => !request.headers.has("Authorization")),
		).toBe(true);
	});
});
