import { afterEach, describe, expect, it, vi } from "vitest";

import { fixtureCatalog } from "@/fixture-policy/catalog";

import {
	canonicalBackendOrigin,
	getPrimaryBackendSnapshot,
	hydratePrimaryBackendProfile,
	installPrimaryBackendTransport,
	resetPrimaryBackendTransportForTests,
	switchPrimaryBackend,
	verifyBackendCandidate,
} from "./primaryBackend";

describe("primary backend transport", () => {
	afterEach(() => {
		resetPrimaryBackendTransportForTests();
		vi.restoreAllMocks();
	});

	it("accepts only page or HTTPS origins without a path", () => {
		expect(canonicalBackendOrigin(window.location.origin)).toBe(
			window.location.origin,
		);
		expect(canonicalBackendOrigin("https://node.example/")).toBe(
			"https://node.example",
		);
		expect(canonicalBackendOrigin("http://node.example")).toBeNull();
		expect(canonicalBackendOrigin("https://node.example/api")).toBeNull();
		expect(canonicalBackendOrigin("https://node.example?x=1")).toBeNull();
	});

	it("tracks mutating API requests until the response settles", async () => {
		let resolveRequest: ((response: Response) => void) | undefined;
		const fetchMock = vi.fn(
			() =>
				new Promise<Response>((resolve) => {
					resolveRequest = resolve;
				}),
		);
		window.fetch = fetchMock as typeof window.fetch;
		installPrimaryBackendTransport();

		const request = window.fetch("/api/admin/nodes", { method: "POST" });
		await Promise.resolve();
		expect(getPrimaryBackendSnapshot().pendingMutations).toBe(1);
		resolveRequest?.(new Response("{}", { status: 200 }));
		await request;
		expect(getPrimaryBackendSnapshot().pendingMutations).toBe(0);
	});

	it("does not mark the backend unavailable for static resource failures", async () => {
		window.fetch = vi.fn(() =>
			Promise.resolve(new Response("missing", { status: 500 })),
		) as typeof window.fetch;
		installPrimaryBackendTransport();

		await window.fetch("/assets/broken.js");

		expect(getPrimaryBackendSnapshot().state).toBe("ready");
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

	it("removes stale nodes and falls back when the primary is removed", async () => {
		hydratePrimaryBackendProfile(fixtureCatalog.cluster.fixture84(), [
			{
				node_id: fixtureCatalog.identifier.nodePrimary(),
				node_name: fixtureCatalog.identifier.nodeNamePrimary(),
				api_base_url: fixtureCatalog.url.primaryApi(),
			},
			{
				node_id: fixtureCatalog.identifier.nodeSecondary(),
				node_name: fixtureCatalog.identifier.nodeNameSecondary(),
				api_base_url: fixtureCatalog.url.secondaryApi(),
			},
		]);
		const recovery = getPrimaryBackendSnapshot().candidates.find(
			(candidate) => candidate.origin === fixtureCatalog.url.secondaryApi(),
		);
		if (!recovery) throw new Error("recovery candidate missing");
		await switchPrimaryBackend({
			...recovery,
			verifiedAt: Date.now(),
		});
		expect(getPrimaryBackendSnapshot().primaryOrigin).toBe(
			fixtureCatalog.url.secondaryApi(),
		);

		hydratePrimaryBackendProfile(fixtureCatalog.cluster.fixture84(), [
			{
				node_id: fixtureCatalog.identifier.nodePrimary(),
				node_name: fixtureCatalog.identifier.nodeNamePrimary(),
				api_base_url: fixtureCatalog.url.primaryApi(),
			},
		]);
		const snapshot = getPrimaryBackendSnapshot();
		expect(snapshot.primaryOrigin).toBe(window.location.origin);
		expect(
			snapshot.candidates.some(
				(candidate) => candidate.origin === fixtureCatalog.url.secondaryApi(),
			),
		).toBe(false);
	});

	it("allows manual switching after the mutation barrier times out without replay", async () => {
		vi.useFakeTimers();
		let resolveMutation: ((response: Response) => void) | undefined;
		const fetchMock = vi.fn(
			() =>
				new Promise<Response>((resolve) => {
					resolveMutation = resolve;
				}),
		);
		window.fetch = fetchMock as typeof window.fetch;
		installPrimaryBackendTransport();
		const mutation = window.fetch("/api/admin/nodes", { method: "POST" });
		await Promise.resolve();
		hydratePrimaryBackendProfile(fixtureCatalog.cluster.fixture84(), [
			{
				node_id: fixtureCatalog.identifier.nodePrimary(),
				node_name: fixtureCatalog.identifier.nodeNamePrimary(),
				api_base_url: fixtureCatalog.url.primaryApi(),
			},
			{
				node_id: fixtureCatalog.identifier.nodeSecondary(),
				node_name: fixtureCatalog.identifier.nodeNameSecondary(),
				api_base_url: fixtureCatalog.url.secondaryApi(),
			},
		]);
		const resultPromise = switchPrimaryBackend({
			origin: fixtureCatalog.url.secondaryApi(),
			nodeId: fixtureCatalog.identifier.nodeSecondary(),
			nodeName: fixtureCatalog.identifier.nodeNameSecondary(),
			verifiedAt: Date.now(),
			lastError: null,
		});
		await vi.advanceTimersByTimeAsync(60_000);
		const result = await resultPromise;
		expect(result.timedOut).toBe(true);
		expect(getPrimaryBackendSnapshot().primaryOrigin).toBe(
			fixtureCatalog.url.secondaryApi(),
		);
		expect(getPrimaryBackendSnapshot().unknownMutationCount).toBe(1);
		expect(fetchMock).toHaveBeenCalledTimes(1);
		resolveMutation?.(new Response("{}", { status: 200 }));
		await mutation;
	});

	it("rewrites SSE requests to the selected backend while keeping page assets local", async () => {
		const fetchMock = vi.fn(
			(_input: RequestInfo | URL, _init?: RequestInit): Promise<Response> =>
				Promise.resolve(
					new Response("event: hello\ndata: {}\n\n", { status: 200 }),
				),
		);
		window.fetch = fetchMock as typeof window.fetch;
		installPrimaryBackendTransport();
		hydratePrimaryBackendProfile(fixtureCatalog.cluster.fixture84(), [
			{
				node_id: fixtureCatalog.identifier.nodePrimary(),
				node_name: fixtureCatalog.identifier.nodeNamePrimary(),
				api_base_url: fixtureCatalog.url.primaryApi(),
			},
			{
				node_id: fixtureCatalog.identifier.nodeSecondary(),
				node_name: fixtureCatalog.identifier.nodeNameSecondary(),
				api_base_url: fixtureCatalog.url.secondaryApi(),
			},
		]);
		await switchPrimaryBackend({
			origin: fixtureCatalog.url.secondaryApi(),
			nodeId: fixtureCatalog.identifier.nodeSecondary(),
			nodeName: fixtureCatalog.identifier.nodeNameSecondary(),
			verifiedAt: Date.now(),
			lastError: null,
		});

		await window.fetch("/api/admin/status/events");
		await window.fetch("/assets/app.js");
		expect(fetchMock.mock.calls[0]?.[0]).toBeInstanceOf(Request);
		expect((fetchMock.mock.calls[0]?.[0] as Request).url).toBe(
			`${fixtureCatalog.url.secondaryApi()}/api/admin/status/events`,
		);
		expect((fetchMock.mock.calls[1]?.[0] as Request).url).toBe(
			`${window.location.origin}/assets/app.js`,
		);
	});
});
