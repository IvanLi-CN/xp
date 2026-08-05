import { describe, expect, it } from "vitest";

import {
	API_COMPATIBILITY_WINDOW,
	RELEASE_COMPATIBILITY_CONTRACTS,
	RELEASE_INVENTORIES,
} from "./releaseInventories";

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
		}
	});
});
