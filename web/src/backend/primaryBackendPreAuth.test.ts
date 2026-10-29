import { fixtureCatalog } from "@/fixture-policy/catalog";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
	getPrimaryBackendSnapshot,
	hydratePrimaryBackendProfile,
	installPrimaryBackendTransport,
	probeBackendCandidate,
	readSwitchCandidateResource,
	resetPrimaryBackendTransportForTests,
	selectPreAuthBackend,
	setAuthenticatedBackendReadiness,
	switchPrimaryBackend,
	verifyBackendCandidate,
} from "./primaryBackend";

describe("primary backend pre-auth recovery", () => {
	afterEach(() => {
		resetPrimaryBackendTransportForTests();
		vi.restoreAllMocks();
	});
	it("blocks credentialed requests after reconnect until identity is rechecked", async () => {
		const fetchMock = vi.fn().mockResolvedValue(new Response("{}"));
		window.fetch = fetchMock as typeof window.fetch;
		installPrimaryBackendTransport();
		hydratePrimaryBackendProfile(fixtureCatalog.cluster.fixture84());
		const snapshot = getPrimaryBackendSnapshot();
		setAuthenticatedBackendReadiness({
			origin: snapshot.primaryOrigin,
			clusterId: snapshot.clusterId,
			ready: true,
		});
		window.dispatchEvent(new Event("offline"));
		window.dispatchEvent(new Event("online"));
		await expect(
			window.fetch("/api/admin/nodes", {
				headers: { Authorization: "Bearer test-token" },
			}),
		).rejects.toThrow("identity verification");
		expect(fetchMock).not.toHaveBeenCalled();
		setAuthenticatedBackendReadiness({
			origin: snapshot.primaryOrigin,
			clusterId: snapshot.clusterId,
			ready: true,
		});
		await window.fetch("/api/admin/nodes", {
			headers: { Authorization: "Bearer test-token" },
		});
		expect(fetchMock).toHaveBeenCalledTimes(1);
	});

	it("preserves the registered name for the current page", () => {
		hydratePrimaryBackendProfile(fixtureCatalog.cluster.fixture84(), [
			{
				node_id: fixtureCatalog.identifier.nodePrimary(),
				node_name: fixtureCatalog.identifier.nodeNamePrimary(),
				api_base_url: fixtureCatalog.url.primaryApi(),
			},
		]);
		expect(getPrimaryBackendSnapshot().candidates).toContainEqual(
			expect.objectContaining({
				origin: window.location.origin,
				nodeId: fixtureCatalog.identifier.nodePrimary(),
				nodeName: fixtureCatalog.identifier.nodeNamePrimary(),
			}),
		);
	});

	it("revalidates page-origin recovery directly while the primary is unavailable", async () => {
		window.localStorage.clear();
		resetPrimaryBackendTransportForTests();
		const clusterId = fixtureCatalog.cluster.fixture84();
		const requests: Array<{ url: string; authorization: string | null }> = [];
		window.fetch = vi.fn((input, init) => {
			const request = new Request(input, init);
			requests.push({
				url: request.url,
				authorization: request.headers.get("Authorization"),
			});
			const path = new URL(request.url).pathname;
			const body =
				path === "/api/health"
					? { status: "ok" }
					: path === "/api/cluster/info"
						? { cluster_id: fixtureCatalog.cluster.fixture84() }
						: path === "/api/capabilities"
							? { release_tag: "v3.22.0" }
							: { items: [] };
			return Promise.resolve(
				new Response(JSON.stringify(body), { status: 200 }),
			);
		}) as typeof window.fetch;
		installPrimaryBackendTransport();
		hydratePrimaryBackendProfile(clusterId, [
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
		const secondary = getPrimaryBackendSnapshot().candidates.find(
			(candidate) => candidate.origin === fixtureCatalog.url.secondaryApi(),
		);
		if (!secondary) throw new Error("secondary candidate missing");
		await switchPrimaryBackend({ ...secondary, verifiedAt: Date.now() });
		setAuthenticatedBackendReadiness({
			origin: secondary.origin,
			clusterId,
			ready: false,
		});
		const page = getPrimaryBackendSnapshot().candidates.find(
			(candidate) => candidate.origin === window.location.origin,
		);
		if (!page) throw new Error("page candidate missing");
		await switchPrimaryBackend(
			{ ...page, verifiedAt: Date.now() },
			{
				revalidate: async () => {
					await readSwitchCandidateResource({
						origin: page.origin,
						clusterId,
						adminToken: "test-token",
						path: "/api/admin/nodes",
					});
					return true;
				},
			},
		);
		expect(
			requests.every(
				(request) => new URL(request.url).origin === window.location.origin,
			),
		).toBe(true);
		expect(
			requests.slice(0, 3).every((request) => request.authorization === null),
		).toBe(true);
		expect(requests.at(-1)?.authorization).toBe("Bearer test-token");
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

	it("probes a candidate publicly before any credentialed request", async () => {
		const requests: Array<{ path: string; authorization: string | null }> = [];
		window.fetch = vi.fn((input, init) => {
			const request = new Request(input, init);
			const url = new URL(request.url);
			requests.push({
				path: url.pathname,
				authorization: request.headers.get("Authorization"),
			});
			if (url.pathname === "/api/health") {
				return Promise.resolve(
					new Response(JSON.stringify({ status: "ok" }), { status: 200 }),
				);
			}
			if (url.pathname === "/api/cluster/info") {
				return Promise.resolve(
					new Response(
						JSON.stringify({
							cluster_id: fixtureCatalog.cluster.fixture84(),
						}),
						{ status: 200 },
					),
				);
			}
			return Promise.resolve(
				new Response(JSON.stringify({ release_tag: "v3.22.0" }), {
					status: 200,
				}),
			);
		}) as typeof window.fetch;
		installPrimaryBackendTransport();

		await probeBackendCandidate({
			origin: fixtureCatalog.url.secondaryApi(),
			clusterId: fixtureCatalog.cluster.fixture84(),
		});

		expect(requests.map((request) => request.path)).toEqual([
			"/api/health",
			"/api/cluster/info",
			"/api/capabilities",
		]);
		expect(requests.every((request) => request.authorization === null)).toBe(
			true,
		);
	});

	it("keeps pre-auth selection out of the durable profile", async () => {
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
		const before = JSON.parse(
			window.localStorage.getItem("xp-primary-backend-profiles") ?? "{}",
		);

		await selectPreAuthBackend(fixtureCatalog.url.secondaryApi());

		expect(getPrimaryBackendSnapshot().primaryOrigin).toBe(
			fixtureCatalog.url.secondaryApi(),
		);
		expect(
			JSON.parse(
				window.localStorage.getItem("xp-primary-backend-profiles") ?? "{}",
			).profiles[fixtureCatalog.cluster.fixture84()].primaryOrigin,
		).toBe(before.profiles[fixtureCatalog.cluster.fixture84()].primaryOrigin);
	});

	it("restores primary and gates pre-auth persistence on inventory hydration", async () => {
		const clusterId = fixtureCatalog.cluster.fixture84();
		const secondary = fixtureCatalog.url.secondaryApi();
		window.localStorage.setItem(
			"xp-primary-backend-profiles",
			JSON.stringify({
				activeClusterId: clusterId,
				profiles: {
					[fixtureCatalog.cluster.fixture84()]: {
						clusterId,
						primaryOrigin: secondary,
						candidates: [
							{
								origin: secondary,
								nodeId: fixtureCatalog.identifier.nodeSecondary(),
								nodeName: fixtureCatalog.identifier.nodeNameSecondary(),
								verifiedAt: Date.now(),
								lastError: null,
							},
						],
						updatedAt: fixtureCatalog.number.value100(),
					},
				},
			}),
		);
		resetPrimaryBackendTransportForTests();
		expect(getPrimaryBackendSnapshot().primaryOrigin).toBe(secondary);

		hydratePrimaryBackendProfile(clusterId);
		await selectPreAuthBackend(window.location.origin);
		const beforeHydration = JSON.parse(
			window.localStorage.getItem("xp-primary-backend-profiles") ?? "{}",
		);
		expect(beforeHydration.profiles[clusterId].primaryOrigin).toBe(secondary);

		hydratePrimaryBackendProfile(clusterId, [
			{
				node_id: fixtureCatalog.identifier.nodePrimary(),
				node_name: fixtureCatalog.identifier.nodeNamePrimary(),
				api_base_url: fixtureCatalog.url.primaryApi(),
			},
		]);
		const afterHydration = JSON.parse(
			window.localStorage.getItem("xp-primary-backend-profiles") ?? "{}",
		);
		expect(afterHydration.profiles[clusterId].primaryOrigin).toBe(
			window.location.origin,
		);
	});

	it("does not persist a selected origin absent from the authenticated inventory", async () => {
		window.localStorage.clear();
		resetPrimaryBackendTransportForTests();
		const clusterId = fixtureCatalog.cluster.fixture84();
		const secondary = fixtureCatalog.url.secondaryApi();
		hydratePrimaryBackendProfile(clusterId, [
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
		const before = window.localStorage.getItem("xp-primary-backend-profiles");
		await selectPreAuthBackend(secondary);
		hydratePrimaryBackendProfile(clusterId, []);
		expect(window.localStorage.getItem("xp-primary-backend-profiles")).toBe(
			before,
		);
	});

	it("reads candidates only from the active cluster profile", () => {
		const activeClusterId = fixtureCatalog.cluster.fixture84();
		const otherClusterId = fixtureCatalog.cluster.fixture95();
		window.localStorage.setItem(
			"xp-primary-backend-profiles",
			JSON.stringify({
				activeClusterId,
				profiles: {
					[fixtureCatalog.cluster.fixture84()]: {
						clusterId: activeClusterId,
						primaryOrigin: fixtureCatalog.url.secondaryApi(),
						candidates: [
							{
								origin: fixtureCatalog.url.secondaryApi(),
								nodeId: fixtureCatalog.identifier.nodeSecondary(),
								nodeName: fixtureCatalog.identifier.nodeNameSecondary(),
								verifiedAt: Date.now(),
								lastError: null,
							},
						],
						updatedAt: fixtureCatalog.number.value100(),
					},
					[fixtureCatalog.cluster.fixture95()]: {
						clusterId: otherClusterId,
						primaryOrigin: fixtureCatalog.url.primaryApi(),
						candidates: [
							{
								origin: fixtureCatalog.url.primaryApi(),
								nodeId: fixtureCatalog.identifier.nodePrimary(),
								nodeName: fixtureCatalog.identifier.nodeNamePrimary(),
								verifiedAt: Date.now(),
								lastError: null,
							},
						],
						updatedAt: fixtureCatalog.number.value100(),
					},
				},
			}),
		);
		resetPrimaryBackendTransportForTests();

		hydratePrimaryBackendProfile(activeClusterId);

		const snapshot = getPrimaryBackendSnapshot();
		expect(snapshot.candidates.map((candidate) => candidate.origin)).toEqual([
			fixtureCatalog.url.secondaryApi(),
			window.location.origin,
		]);
		expect(snapshot.candidates).not.toContainEqual(
			expect.objectContaining({
				nodeId: fixtureCatalog.identifier.nodePrimary(),
			}),
		);
	});
});
