import { afterEach, describe, expect, it, vi } from "vitest";

import { fixtureCatalog } from "@/fixture-policy/catalog";

import {
	canonicalBackendOrigin,
	currentWebReleaseVersion,
	getPrimaryBackendSnapshot,
	getStaticRuntimePolicyOrigins,
	hydrateBackendVersions,
	hydratePrimaryBackendProfile,
	hydrateStaticRuntimePolicy,
	installPrimaryBackendTransport,
	normalizeReleaseVersion,
	recognizedReleaseVersion,
	resetPrimaryBackendTransportForTests,
	subscribePrimaryBackend,
	switchPrimaryBackend,
	verifyBackendCandidate,
	withPrimaryBackendMutation,
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

	it("stages policy origins and preserves temporarily unavailable candidates", () => {
		hydrateStaticRuntimePolicy(
			fixtureCatalog.cluster.fixture84(),
			[fixtureCatalog.url.secondaryApi(), fixtureCatalog.url.primaryApi()],
			{
				policyId: "rcp-staged",
				nodeStates: [
					{
						origin: fixtureCatalog.url.secondaryApi(),
						state: "allowed",
					},
					{
						origin: fixtureCatalog.url.primaryApi(),
						state: "temporarily_unavailable",
					},
				],
			},
		);
		const snapshot = getPrimaryBackendSnapshot();
		expect(
			snapshot.candidates.find(
				(candidate) => candidate.origin === fixtureCatalog.url.secondaryApi(),
			)?.runtimeState,
		).toBe("staged");
		expect(
			snapshot.candidates.find(
				(candidate) => candidate.origin === fixtureCatalog.url.primaryApi(),
			)?.runtimeState,
		).toBe("temporarily_unavailable");
		expect(getStaticRuntimePolicyOrigins()).toEqual([
			"https://101-xp.ivanli.cc",
		]);
	});

	it("falls back from a selected backend when it becomes temporarily unavailable", async () => {
		hydrateStaticRuntimePolicy(
			fixtureCatalog.cluster.fixture84(),
			[fixtureCatalog.url.primaryApi()],
			{
				policyId: "rcp-active",
				nodeStates: [
					{
						origin: fixtureCatalog.url.primaryApi(),
						state: "allowed",
					},
				],
				activate: true,
			},
		);
		const primary = getPrimaryBackendSnapshot().candidates.find(
			(candidate) => candidate.origin === fixtureCatalog.url.primaryApi(),
		);
		if (!primary) throw new Error("active candidate missing");
		await switchPrimaryBackend({ ...primary, verifiedAt: Date.now() });
		expect(getPrimaryBackendSnapshot().primaryOrigin).toBe(
			fixtureCatalog.url.primaryApi(),
		);

		hydrateStaticRuntimePolicy(
			fixtureCatalog.cluster.fixture84(),
			[fixtureCatalog.url.primaryApi()],
			{
				policyId: "rcp-unavailable",
				nodeStates: [
					{
						origin: fixtureCatalog.url.primaryApi(),
						state: "temporarily_unavailable",
					},
				],
				candidateOrigins: [
					"https://101-xp.ivanli.cc",
					fixtureCatalog.url.primaryApi(),
				],
			},
		);
		const snapshot = getPrimaryBackendSnapshot();
		expect(snapshot.primaryOrigin).toBe("https://101-xp.ivanli.cc");
		expect(getStaticRuntimePolicyOrigins()).toEqual([
			"https://101-xp.ivanli.cc",
		]);
		expect(
			snapshot.candidates.find(
				(candidate) => candidate.origin === fixtureCatalog.url.primaryApi(),
			)?.runtimeState,
		).toBe("temporarily_unavailable");
	});

	it("attaches service versions by registered node id and preserves unknowns", () => {
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
			{
				node_id: fixtureCatalog.identifier.nodeTertiary(),
				node_name: fixtureCatalog.identifier.nodeNameTertiary(),
				api_base_url: fixtureCatalog.url.tertiaryApi(),
			},
		]);

		hydrateBackendVersions([
			{
				node_id: fixtureCatalog.identifier.nodePrimary(),
				xp_version: "v3.22.5",
				error: null,
			},
			{
				node_id: fixtureCatalog.identifier.nodeSecondary(),
				xp_version: null,
				error: "unreachable",
			},
			{
				node_id: fixtureCatalog.identifier.nodeTertiary(),
				xp_version: "v3.21.4",
				error: null,
			},
		]);

		const snapshot = getPrimaryBackendSnapshot();
		expect(snapshot.candidates).toEqual(
			expect.arrayContaining([
				expect.objectContaining({
					nodeId: fixtureCatalog.identifier.nodePrimary(),
					serviceVersion: "3.22.5",
					versionState: "ready",
				}),
				expect.objectContaining({
					nodeId: fixtureCatalog.identifier.nodeSecondary(),
					serviceVersion: null,
					versionState: "unavailable",
					lastError: "unreachable",
				}),
				expect.objectContaining({
					nodeId: fixtureCatalog.identifier.nodeTertiary(),
					serviceVersion: "3.21.4",
					versionState: "ready",
				}),
			]),
		);
		expect(normalizeReleaseVersion(" v3.21.4 ")).toBe("3.21.4");
		expect(recognizedReleaseVersion("3.22.5-buildhash")).toBeNull();
		expect(currentWebReleaseVersion()).toBe("3.22.5");

		hydrateBackendVersions([]);
		expect(getPrimaryBackendSnapshot().candidates).toEqual(
			expect.arrayContaining([
				expect.objectContaining({
					nodeId: fixtureCatalog.identifier.nodePrimary(),
					serviceVersion: null,
					versionState: "unknown",
					lastError: "unknown",
				}),
			]),
		);
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

	it("waits for caller-owned transactions before switching backends", async () => {
		window.localStorage.clear();
		resetPrimaryBackendTransportForTests();
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
		let resolveTransaction: (() => void) | undefined;
		const transaction = withPrimaryBackendMutation(
			() =>
				new Promise<void>((resolve) => {
					resolveTransaction = resolve;
				}),
		);
		await Promise.resolve();
		expect(getPrimaryBackendSnapshot().pendingMutations).toBe(1);

		const switchPromise = switchPrimaryBackend({
			origin: fixtureCatalog.url.secondaryApi(),
			nodeId: fixtureCatalog.identifier.nodeSecondary(),
			nodeName: fixtureCatalog.identifier.nodeNameSecondary(),
			verifiedAt: Date.now(),
			lastError: null,
		});
		await Promise.resolve();
		expect(getPrimaryBackendSnapshot().primaryOrigin).toBe(
			window.location.origin,
		);

		resolveTransaction?.();
		await transaction;
		await switchPromise;
		expect(getPrimaryBackendSnapshot().primaryOrigin).toBe(
			fixtureCatalog.url.secondaryApi(),
		);
	});

	it("rechecks the barrier when a subscriber starts a mutation during drain", async () => {
		window.localStorage.clear();
		resetPrimaryBackendTransportForTests();
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
		let resolveFirst: (() => void) | undefined;
		const reentrantResolvers: Array<() => void> = [];
		const reentrantTransactions: Array<Promise<void>> = [];
		const first = withPrimaryBackendMutation(
			() =>
				new Promise<void>((resolve) => {
					resolveFirst = resolve;
				}),
		);
		await Promise.resolve();
		const unsubscribe = subscribePrimaryBackend((snapshot) => {
			if (
				snapshot.state !== "switching" ||
				snapshot.pendingMutations !== 0 ||
				reentrantTransactions.length >= 2
			) {
				return;
			}
			const transaction = withPrimaryBackendMutation(
				() =>
					new Promise<void>((resolve) => {
						reentrantResolvers.push(resolve);
					}),
			);
			reentrantTransactions.push(transaction);
		});

		const switchPromise = switchPrimaryBackend({
			origin: fixtureCatalog.url.secondaryApi(),
			nodeId: fixtureCatalog.identifier.nodeSecondary(),
			nodeName: fixtureCatalog.identifier.nodeNameSecondary(),
			verifiedAt: Date.now(),
			lastError: null,
		});
		await Promise.resolve();
		resolveFirst?.();
		await first;
		await Promise.resolve();
		expect(reentrantTransactions).toHaveLength(1);
		expect(getPrimaryBackendSnapshot().primaryOrigin).toBe(
			window.location.origin,
		);
		reentrantResolvers.shift()?.();
		await reentrantTransactions[0];
		await Promise.resolve();
		expect(reentrantTransactions).toHaveLength(2);
		expect(getPrimaryBackendSnapshot().primaryOrigin).toBe(
			window.location.origin,
		);
		reentrantResolvers.shift()?.();
		await reentrantTransactions[1];
		await switchPromise;
		unsubscribe();
		expect(getPrimaryBackendSnapshot().primaryOrigin).toBe(
			fixtureCatalog.url.secondaryApi(),
		);
	});

	it("rejects a candidate removed while the mutation barrier is draining", async () => {
		window.localStorage.clear();
		resetPrimaryBackendTransportForTests();
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
		let resolveTransaction: (() => void) | undefined;
		const transaction = withPrimaryBackendMutation(
			() =>
				new Promise<void>((resolve) => {
					resolveTransaction = resolve;
				}),
		);
		await Promise.resolve();
		const switchPromise = switchPrimaryBackend({
			origin: fixtureCatalog.url.secondaryApi(),
			nodeId: fixtureCatalog.identifier.nodeSecondary(),
			nodeName: fixtureCatalog.identifier.nodeNameSecondary(),
			verifiedAt: Date.now(),
			lastError: null,
		});
		await Promise.resolve();
		hydratePrimaryBackendProfile(fixtureCatalog.cluster.fixture84(), [
			{
				node_id: fixtureCatalog.identifier.nodePrimary(),
				node_name: fixtureCatalog.identifier.nodeNamePrimary(),
				api_base_url: fixtureCatalog.url.primaryApi(),
			},
		]);
		resolveTransaction?.();
		await transaction;
		await expect(switchPromise).rejects.toThrow(
			"no longer available for switching",
		);
		expect(getPrimaryBackendSnapshot().primaryOrigin).toBe(
			window.location.origin,
		);
		expect(getPrimaryBackendSnapshot().state).toBe("ready");
	});

	it("keeps switching state while inventory refreshes during the barrier", async () => {
		window.localStorage.clear();
		resetPrimaryBackendTransportForTests();
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
		let resolveTransaction: (() => void) | undefined;
		const transaction = withPrimaryBackendMutation(
			() =>
				new Promise<void>((resolve) => {
					resolveTransaction = resolve;
				}),
		);
		await Promise.resolve();

		const switchPromise = switchPrimaryBackend({
			origin: fixtureCatalog.url.secondaryApi(),
			nodeId: fixtureCatalog.identifier.nodeSecondary(),
			nodeName: fixtureCatalog.identifier.nodeNameSecondary(),
			verifiedAt: Date.now(),
			lastError: null,
		});
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
		hydrateStaticRuntimePolicy(fixtureCatalog.cluster.fixture84(), [
			fixtureCatalog.url.primaryApi(),
			fixtureCatalog.url.secondaryApi(),
		]);
		expect(getPrimaryBackendSnapshot().state).toBe("switching");

		resolveTransaction?.();
		await transaction;
		await switchPromise;
		expect(getPrimaryBackendSnapshot().state).toBe("ready");
	});

	it("restores the ready state when refreshed directory validation fails", async () => {
		window.localStorage.clear();
		resetPrimaryBackendTransportForTests();
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

		await expect(
			switchPrimaryBackend(
				{
					origin: fixtureCatalog.url.secondaryApi(),
					nodeId: fixtureCatalog.identifier.nodeSecondary(),
					nodeName: fixtureCatalog.identifier.nodeNameSecondary(),
					verifiedAt: Date.now(),
					lastError: null,
				},
				{ revalidate: async () => false },
			),
		).rejects.toThrow("directory is no longer available");
		expect(getPrimaryBackendSnapshot().state).toBe("ready");
		expect(getPrimaryBackendSnapshot().primaryOrigin).toBe(
			window.location.origin,
		);
	});

	it("passes the selected candidate to revalidation", async () => {
		window.localStorage.clear();
		resetPrimaryBackendTransportForTests();
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
		let revalidatedOrigin: string | undefined;

		await switchPrimaryBackend(
			{
				origin: fixtureCatalog.url.secondaryApi(),
				nodeId: fixtureCatalog.identifier.nodeSecondary(),
				nodeName: fixtureCatalog.identifier.nodeNameSecondary(),
				verifiedAt: Date.now(),
				lastError: null,
			},
			{
				revalidate: async (candidate) => {
					revalidatedOrigin = candidate.origin;
					return true;
				},
			},
		);

		expect(revalidatedOrigin).toBe(fixtureCatalog.url.secondaryApi());
	});

	it("drains mutations that start during candidate revalidation", async () => {
		window.localStorage.clear();
		resetPrimaryBackendTransportForTests();
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
		let resolveTransaction: (() => void) | undefined;
		let resolveRevalidation: ((value: boolean) => void) | undefined;
		const switchPromise = switchPrimaryBackend(
			{
				origin: fixtureCatalog.url.secondaryApi(),
				nodeId: fixtureCatalog.identifier.nodeSecondary(),
				nodeName: fixtureCatalog.identifier.nodeNameSecondary(),
				verifiedAt: Date.now(),
				lastError: null,
			},
			{
				revalidate: async () => {
					void withPrimaryBackendMutation(
						() =>
							new Promise<void>((resolve) => {
								resolveTransaction = resolve;
							}),
					);
					return new Promise<boolean>((resolve) => {
						resolveRevalidation = resolve;
					});
				},
			},
		);
		await Promise.resolve();
		resolveRevalidation?.(true);
		await Promise.resolve();
		expect(getPrimaryBackendSnapshot().pendingMutations).toBe(1);
		expect(getPrimaryBackendSnapshot().primaryOrigin).toBe(
			window.location.origin,
		);
		resolveTransaction?.();
		await switchPromise;
		expect(getPrimaryBackendSnapshot().primaryOrigin).toBe(
			fixtureCatalog.url.secondaryApi(),
		);
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
