import { describe, expect, it } from "vitest";

import { AdminNodesRuntimeResponseSchema } from "./adminNodeRuntime";
import { AdminNodesResponseSchema } from "./adminNodes";
import { ClusterInfoResponseSchema } from "./clusterInfo";
import { HealthResponseSchema } from "./health";
import {
	API_COMPATIBILITY_WINDOW,
	RELEASE_API_FIXTURES,
	RELEASE_COMPATIBILITY_CONTRACTS,
	RELEASE_INVENTORIES,
} from "./releaseInventories";
import { VersionCheckResponseSchema } from "./versionCheck";

describe("immutable release inventories", () => {
	it("pins the latest stable tag and source commit for every compatibility minor", () => {
		expect(RELEASE_INVENTORIES.map((inventory) => inventory.minor)).toEqual(
			API_COMPATIBILITY_WINDOW,
		);
		expect(
			RELEASE_INVENTORIES.map((inventory) => inventory.releaseTag),
		).toEqual(["v3.22.5", "v3.21.11", "v3.20.3"]);
		expect(
			RELEASE_INVENTORIES.map((inventory) => inventory.sourceCommit),
		).toEqual([
			"d7e1e652fbd5fa07442bd960894764a5b81ef3bc",
			"8cf9564f366cb260b35106396840cbe3ed903c75",
			"aee855e41f63af0c99f296259b038cda73e24ae9",
		]);
	});

	it("contains immutable per-minor routes and tolerant response contracts", () => {
		for (const inventory of RELEASE_INVENTORIES) {
			expect(inventory.webCallsites).toContain("adminStatusEvents");
			expect(inventory.apiRoutes).toContain("GET /api/admin/status/events");
			expect(inventory.apiRoutes).toContain("GET /events");
			expect(inventory.responseSchemas["GET /api/health"]).toContain("status");
			expect(inventory.fingerprint["/api/health"]).toContain("status");
		}
		const current = RELEASE_INVENTORIES[0];
		const legacy = RELEASE_INVENTORIES[1];
		expect(current?.apiRoutes).toContain("GET /api/admin/mesh/status");
		expect(legacy?.apiRoutes).not.toContain("GET /api/admin/mesh/status");
		expect(current?.responseSchemas["GET /api/admin/status/events"]).toContain(
			"mesh_revision",
		);
		expect(
			legacy?.responseSchemas["GET /api/admin/status/events"],
		).not.toContain("mesh_revision");
	});

	it("executes both compatibility directions for every pinned inventory", () => {
		expect(RELEASE_COMPATIBILITY_CONTRACTS).toHaveLength(6);
		for (const fixture of RELEASE_API_FIXTURES) {
			const inventory = RELEASE_INVENTORIES.find(
				(candidate) => candidate.releaseTag === fixture.releaseTag,
			);
			expect(inventory).toBeDefined();
			if (!inventory) continue;

			const requestRoutes = fixture.requests.map(
				(request) => `${request.method} ${request.path}`,
			);
			expect(new Set(requestRoutes)).toEqual(new Set(inventory.apiRoutes));
			for (const request of fixture.requests) {
				expect(request.expectedStatus).toBe(200);
			}

			for (const response of fixture.responses) {
				const fields = inventory.responseSchemas[response.route];
				expect(fields).toBeDefined();
				if (!fields) continue;
				if (response.contentType === "text/plain") {
					expect(fields).toContain("text/plain");
					continue;
				}

				if (response.route === "GET /api/health") {
					HealthResponseSchema.parse(response.body);
				} else if (response.route === "GET /api/cluster/info") {
					ClusterInfoResponseSchema.parse(response.body);
				} else if (response.route === "GET /api/admin/nodes") {
					AdminNodesResponseSchema.parse(response.body);
				} else if (response.route === "GET /api/admin/nodes/runtime") {
					AdminNodesRuntimeResponseSchema.parse(response.body);
				} else if (response.route === "GET /api/version/check") {
					VersionCheckResponseSchema.parse(response.body);
				}

				const body = response.body as Record<string, unknown>;
				for (const field of fields) {
					if (response.route === "GET /api/admin/status/events") {
						const eventPath =
							field === "hello" || field === "snapshot"
								? field
								: `snapshot.${field}`;
						expect(body).toHaveProperty(eventPath);
					} else {
						expect(body).toHaveProperty(field);
					}
				}
			}
		}

		for (const inventory of RELEASE_INVENTORIES) {
			const newWebContract = RELEASE_COMPATIBILITY_CONTRACTS.find(
				(contract) =>
					contract.direction === "new-web-to-legacy-api" &&
					contract.serverReleaseTag === inventory.releaseTag,
			);
			const legacyWebContract = RELEASE_COMPATIBILITY_CONTRACTS.find(
				(contract) =>
					contract.direction === "legacy-web-to-new-api" &&
					contract.consumerReleaseTag === inventory.releaseTag,
			);
			expect(newWebContract?.apiRoutes).toEqual(inventory.apiRoutes);
			expect(legacyWebContract?.apiRoutes).toEqual(inventory.apiRoutes);
			expect(newWebContract?.webCallsites).toEqual(inventory.webCallsites);
			expect(legacyWebContract?.webCallsites).toEqual(inventory.webCallsites);
			expect(newWebContract?.responseSchemas).toEqual(
				inventory.responseSchemas,
			);
			expect(legacyWebContract?.responseSchemas).toEqual(
				inventory.responseSchemas,
			);
		}
	});
});
