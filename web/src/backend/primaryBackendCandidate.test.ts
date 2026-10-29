import { afterEach, describe, expect, it, vi } from "vitest";

import { fixtureCatalog } from "@/fixture-policy/catalog";
import {
	clearPrimaryBackendDirectory,
	installPrimaryBackendTransport,
	resetPrimaryBackendTransportForTests,
	verifyBackendCandidate,
} from "./primaryBackend";

describe("backend candidate verification", () => {
	afterEach(() => {
		resetPrimaryBackendTransportForTests();
		vi.restoreAllMocks();
	});

	it("does not send a token before the candidate cluster matches", async () => {
		const requests: Array<{ url: string; authorization: string | null }> = [];
		window.fetch = vi.fn((input, init) => {
			const request = new Request(input, init);
			requests.push({
				url: request.url,
				authorization: request.headers.get("Authorization"),
			});
			if (request.url.endsWith("/api/health")) {
				return Promise.resolve(
					new Response(JSON.stringify({ status: "ok" }), { status: 200 }),
				);
			}
			return Promise.resolve(
				new Response(
					JSON.stringify({ cluster_id: fixtureCatalog.cluster.fixture95() }),
					{
						status: 200,
					},
				),
			);
		}) as typeof window.fetch;
		installPrimaryBackendTransport();

		await expect(
			verifyBackendCandidate({
				origin: fixtureCatalog.url.secondaryApi(),
				clusterId: fixtureCatalog.cluster.fixture84(),
				adminToken: "secret-token",
			}),
		).rejects.toThrow("different cluster");
		expect(requests).toHaveLength(2);
		expect(requests.every((request) => request.authorization === null)).toBe(
			true,
		);
	});

	it("does not send a token if the session ends during public verification", async () => {
		let finishCapabilities!: (response: Response) => void;
		const requests: Request[] = [];
		window.fetch = vi.fn((input, init) => {
			const request = new Request(input, init);
			requests.push(request);
			if (request.url.endsWith("/api/health")) {
				return Promise.resolve(new Response(JSON.stringify({ status: "ok" })));
			}
			if (request.url.endsWith("/api/cluster/info")) {
				return Promise.resolve(
					new Response(
						JSON.stringify({ cluster_id: fixtureCatalog.cluster.fixture84() }),
					),
				);
			}
			return new Promise<Response>((resolve) => {
				finishCapabilities = resolve;
			});
		}) as typeof window.fetch;
		const pending = verifyBackendCandidate({
			origin: fixtureCatalog.url.secondaryApi(),
			clusterId: fixtureCatalog.cluster.fixture84(),
			adminToken: "secret-token",
		});
		await vi.waitFor(() => expect(finishCapabilities).toBeDefined());
		clearPrimaryBackendDirectory();
		finishCapabilities(
			new Response(JSON.stringify({ release_tag: "v3.22.0" })),
		);
		await expect(pending).rejects.toThrow("session changed");
		expect(
			requests.every((request) => !request.headers.has("Authorization")),
		).toBe(true);
	});

	it("accepts a compatible registered candidate after authenticated inventory", async () => {
		const requests: Array<{ url: string; authorization: string | null }> = [];
		window.fetch = vi.fn((input, init) => {
			const request = new Request(input, init);
			requests.push({
				url: request.url,
				authorization: request.headers.get("Authorization"),
			});
			if (request.url.endsWith("/api/health")) {
				return Promise.resolve(
					new Response(JSON.stringify({ status: "ok" }), { status: 200 }),
				);
			}
			if (request.url.endsWith("/api/cluster/info")) {
				return Promise.resolve(
					new Response(
						JSON.stringify({ cluster_id: fixtureCatalog.cluster.fixture84() }),
						{
							status: 200,
						},
					),
				);
			}
			if (request.url.endsWith("/api/capabilities")) {
				return Promise.resolve(
					new Response(JSON.stringify({ release_tag: "v3.22.0" }), {
						status: 200,
					}),
				);
			}
			return Promise.resolve(
				new Response(
					JSON.stringify({
						items: [
							{
								node_id: fixtureCatalog.identifier.nodeSecondary(),
								node_name: fixtureCatalog.identifier.nodeNameSecondary(),
								api_base_url: fixtureCatalog.url.secondaryApi(),
							},
						],
					}),
					{ status: 200 },
				),
			);
		}) as typeof window.fetch;
		installPrimaryBackendTransport();

		const candidate = await verifyBackendCandidate({
			origin: fixtureCatalog.url.secondaryApi(),
			clusterId: fixtureCatalog.cluster.fixture84(),
			adminToken: "secret-token",
		});
		expect(candidate.nodeId).toBe(fixtureCatalog.identifier.nodeSecondary());
		expect(requests.at(-1)?.authorization).toBe("Bearer secret-token");
	});
});
