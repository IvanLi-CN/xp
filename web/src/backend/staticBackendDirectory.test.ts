// @vitest-environment jsdom
// @vitest-environment-options {"url":"https://xp.ivanli.cc"}
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { clearAdminToken, writeAdminToken } from "@/components/auth";
import { fixtureCatalog } from "@/fixture-policy/catalog";
import { writeAppliedRuntimePolicyId } from "@/offline/runtimePolicySession";
import { clearServiceWorkerRuntimePolicy } from "@/offline/serviceWorkerPolicy";

vi.mock("@/offline/serviceWorkerPolicy", () => ({
	clearServiceWorkerRuntimePolicy: vi.fn(),
}));
import {
	getPrimaryBackendSnapshot,
	getStaticRuntimePolicyOrigins,
	hydrateBackendVersions,
	hydratePrimaryBackendProfile,
	hydrateStaticRuntimePolicy,
	resetPrimaryBackendTransportForTests,
} from "./primaryBackend";

const cluster = "cluster-static-test";
const shared = fixtureCatalog.url.nodeA();
const inventory = [
	{
		node_id: fixtureCatalog.nodeId.fixture17(),
		node_name: fixtureCatalog.nodeName.fixture18(),
		api_base_url: fixtureCatalog.url.nodeA(),
	},
	{
		node_id: fixtureCatalog.nodeId.fixture32(),
		node_name: fixtureCatalog.nodeName.fixture33(),
		api_base_url: fixtureCatalog.url.nodeA(),
	},
	{
		node_id: fixtureCatalog.nodeId.fixture36(),
		node_name: fixtureCatalog.nodeName.fixture37(),
		api_base_url: fixtureCatalog.url.origin443(),
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
			window.localStorage.getItem("xp-primary-backend-profiles"),
		).toBeNull();
		expect(
			getPrimaryBackendSnapshot().candidates.map((item) => item.nodeId),
		).toEqual([
			fixtureCatalog.nodeId.fixture17(),
			fixtureCatalog.nodeId.fixture32(),
			fixtureCatalog.nodeId.fixture36(),
		]);
		hydrateStaticRuntimePolicy(cluster, [], {
			policyId: "second",
			activate: true,
		});
		const rows = getPrimaryBackendSnapshot().candidates;
		expect(rows.map((item) => item.nodeId)).toEqual([
			fixtureCatalog.nodeId.fixture17(),
			fixtureCatalog.nodeId.fixture32(),
			fixtureCatalog.nodeId.fixture36(),
		]);
		hydratePrimaryBackendProfile(cluster);
		expect(
			getPrimaryBackendSnapshot().candidates.map((item) => item.nodeId),
		).toEqual([
			fixtureCatalog.nodeId.fixture17(),
			fixtureCatalog.nodeId.fixture32(),
			fixtureCatalog.nodeId.fixture36(),
		]);
		expect(rows.every((item) => item.switchable === false)).toBe(true);
		expect(getStaticRuntimePolicyOrigins()).toEqual([
			"https://101-xp.ivanli.cc",
		]);
		hydratePrimaryBackendProfile(cluster, [inventory[1]]);
		expect(
			getPrimaryBackendSnapshot().candidates.map((item) => item.nodeId),
		).toEqual([fixtureCatalog.nodeId.fixture32()]);
	});

	it("clears previous topology on cluster change and logout", () => {
		writeAdminToken("test-token");
		hydratePrimaryBackendProfile(cluster, inventory);
		hydratePrimaryBackendProfile("other-cluster");
		expect(
			getPrimaryBackendSnapshot().candidates.some(
				(item) => item.nodeId === fixtureCatalog.nodeId.fixture17(),
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
			{
				node_id: fixtureCatalog.nodeId.fixture17(),
				xp_version: "3.45.0",
				error: null,
			},
			{
				node_id: fixtureCatalog.nodeId.fixture32(),
				xp_version: "3.46.0",
				error: null,
			},
		]);
		hydrateStaticRuntimePolicy(cluster, [shared], {
			policyId: "second",
			activate: true,
		});
		hydratePrimaryBackendProfile(cluster, inventory);
		const rows = getPrimaryBackendSnapshot().candidates;
		expect(
			rows.find((item) => item.nodeId === fixtureCatalog.nodeId.fixture17())
				?.serviceVersion,
		).toBe("3.45.0");
		expect(
			rows.find((item) => item.nodeId === fixtureCatalog.nodeId.fixture32())
				?.serviceVersion,
		).toBe("3.46.0");
	});

	it("clears the directory when another tab signs out", () => {
		writeAdminToken("test-token");
		hydratePrimaryBackendProfile(cluster, inventory);
		writeAppliedRuntimePolicyId("policy-test");
		vi.mocked(clearServiceWorkerRuntimePolicy).mockClear();
		window.dispatchEvent(
			new StorageEvent("storage", {
				key: "xp_admin_token",
				oldValue: "test-token",
				newValue: null,
			}),
		);
		expect(getPrimaryBackendSnapshot().candidates).toEqual([]);
		expect(
			window.sessionStorage.getItem("xp-runtime-policy-applied"),
		).toBeNull();
		expect(clearServiceWorkerRuntimePolicy).toHaveBeenCalledOnce();
	});

	it("clears the worker grant and applied policy when the token is cleared", () => {
		writeAdminToken("test-token");
		writeAppliedRuntimePolicyId("policy-test");
		vi.mocked(clearServiceWorkerRuntimePolicy).mockClear();
		clearAdminToken();
		expect(
			window.sessionStorage.getItem("xp-runtime-policy-applied"),
		).toBeNull();
		expect(clearServiceWorkerRuntimePolicy).toHaveBeenCalledOnce();
	});
});
