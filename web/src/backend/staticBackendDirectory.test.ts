// @vitest-environment jsdom
// @vitest-environment-options {"url":"https://xp.ivanli.cc"}
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { clearAdminToken, writeAdminToken } from "@/components/auth";
import {
	getPrimaryBackendSnapshot,
	getStaticRuntimePolicyOrigins,
	hydrateBackendVersions,
	hydratePrimaryBackendProfile,
	hydrateStaticRuntimePolicy,
	resetPrimaryBackendTransportForTests,
} from "./primaryBackend";

const cluster = "cluster-static-test";
const shared = "https://shared.xp.example";
const inventory = [
	{ node_id: "a", node_name: "A", api_base_url: shared },
	{ node_id: "b", node_name: "B", api_base_url: shared },
	{
		node_id: "bad",
		node_name: "Bad",
		api_base_url: "http://insecure.xp.example",
	},
];

describe("static backend directory", () => {
	beforeEach(() => {
		window.localStorage.clear();
		resetPrimaryBackendTransportForTests();
	});
	afterEach(() => resetPrimaryBackendTransportForTests());

	it("retains rows through policy refresh and replaces them on inventory success", () => {
		hydrateStaticRuntimePolicy(cluster, [shared], {
			policyId: "first",
			activate: true,
		});
		hydratePrimaryBackendProfile(cluster, inventory);
		expect(
			getPrimaryBackendSnapshot().candidates.map((item) => item.nodeId),
		).toEqual(["a", "b", "bad"]);
		hydrateStaticRuntimePolicy(cluster, [], {
			policyId: "second",
			activate: true,
		});
		const rows = getPrimaryBackendSnapshot().candidates;
		expect(rows.map((item) => item.nodeId)).toEqual(["a", "b", "bad"]);
		hydratePrimaryBackendProfile(cluster);
		expect(
			getPrimaryBackendSnapshot().candidates.map((item) => item.nodeId),
		).toEqual(["a", "b", "bad"]);
		expect(rows.every((item) => item.switchable === false)).toBe(true);
		expect(getStaticRuntimePolicyOrigins()).toEqual([
			"https://101-xp.ivanli.cc",
		]);
		hydratePrimaryBackendProfile(cluster, [inventory[1]]);
		expect(
			getPrimaryBackendSnapshot().candidates.map((item) => item.nodeId),
		).toEqual(["b"]);
	});

	it("clears previous topology on cluster change and logout", () => {
		writeAdminToken("test-token");
		hydratePrimaryBackendProfile(cluster, inventory);
		hydratePrimaryBackendProfile("other-cluster");
		expect(
			getPrimaryBackendSnapshot().candidates.some(
				(item) => item.nodeId === "a",
			),
		).toBe(false);
		hydratePrimaryBackendProfile("other-cluster", [inventory[0]]);
		clearAdminToken();
		expect(getPrimaryBackendSnapshot().candidates).toEqual([]);
		expect(
			window.localStorage.getItem("xp-primary-backend-profiles"),
		).toBeNull();
	});

	it("preserves each shared-origin node's version across inventory refreshes", () => {
		hydrateStaticRuntimePolicy(cluster, [shared], {
			policyId: "first",
			activate: true,
		});
		hydratePrimaryBackendProfile(cluster, inventory);
		hydrateBackendVersions([
			{ node_id: "a", xp_version: "3.45.0", error: null },
			{ node_id: "b", xp_version: "3.46.0", error: null },
		]);
		hydrateStaticRuntimePolicy(cluster, [shared], {
			policyId: "second",
			activate: true,
		});
		hydratePrimaryBackendProfile(cluster, inventory);
		const rows = getPrimaryBackendSnapshot().candidates;
		expect(rows.find((item) => item.nodeId === "a")?.serviceVersion).toBe(
			"3.45.0",
		);
		expect(rows.find((item) => item.nodeId === "b")?.serviceVersion).toBe(
			"3.46.0",
		);
	});

	it("clears the directory when another tab signs out", () => {
		writeAdminToken("test-token");
		hydratePrimaryBackendProfile(cluster, inventory);
		window.dispatchEvent(
			new StorageEvent("storage", {
				key: "xp_admin_token",
				oldValue: "test-token",
				newValue: null,
			}),
		);
		expect(getPrimaryBackendSnapshot().candidates).toEqual([]);
	});
});
