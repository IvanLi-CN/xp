import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { fixtureCatalog } from "@/fixture-policy/catalog";
import type { RuntimePolicy } from "../api/runtimePolicy";
import {
	getPrimaryBackendSnapshot,
	getStaticRuntimePolicyOrigins,
	resetPrimaryBackendTransportForTests,
} from "../backend/primaryBackend";
import {
	readAppliedRuntimePolicyId,
	writeAppliedRuntimePolicyId,
} from "./runtimePolicySession";
import { useStaticRuntimePolicy } from "./useStaticRuntimePolicy";

const POLICY: RuntimePolicy = {
	policy_id: "rcp-next",
	cluster_id: fixtureCatalog.cluster.fixture84(),
	issued_at: "2099-01-01T00:00:00.000Z",
	expires_at: "2099-01-01T00:10:00.000Z",
	api_origins: [fixtureCatalog.url.primaryApi()],
	node_states: [
		{
			origin: fixtureCatalog.url.primaryApi(),
			state: "allowed",
		},
		{
			origin: fixtureCatalog.url.secondaryApi(),
			state: "temporarily_unavailable",
		},
	],
};

describe("useStaticRuntimePolicy", () => {
	beforeEach(() => {
		window.sessionStorage.clear();
		resetPrimaryBackendTransportForTests();
	});

	afterEach(() => {
		window.sessionStorage.clear();
		resetPrimaryBackendTransportForTests();
	});

	it("stages policy changes without activating a new backend", async () => {
		renderHook(() =>
			useStaticRuntimePolicy({
				enabled: true,
				clusterId: POLICY.cluster_id,
				policy: POLICY,
			}),
		);

		await waitFor(() =>
			expect(
				getPrimaryBackendSnapshot().candidates.find(
					(candidate) => candidate.origin === fixtureCatalog.url.primaryApi(),
				)?.runtimeState,
			).toBe("staged"),
		);
		expect(readAppliedRuntimePolicyId()).toBeNull();
		expect(getStaticRuntimePolicyOrigins()).toEqual([
			"https://101-xp.ivanli.cc",
		]);
		expect(
			getPrimaryBackendSnapshot().candidates.find(
				(candidate) => candidate.origin === fixtureCatalog.url.secondaryApi(),
			)?.runtimeState,
		).toBe("temporarily_unavailable");
	});

	it("activates only after the applied policy id is recorded", async () => {
		const { rerender } = renderHook(
			({ policy }: { policy: RuntimePolicy }) =>
				useStaticRuntimePolicy({
					enabled: true,
					clusterId: POLICY.cluster_id,
					policy,
				}),
			{ initialProps: { policy: POLICY } },
		);
		await waitFor(() =>
			expect(getStaticRuntimePolicyOrigins()).toEqual([
				"https://101-xp.ivanli.cc",
			]),
		);

		act(() => writeAppliedRuntimePolicyId(POLICY.policy_id));
		rerender({ policy: { ...POLICY } });
		await waitFor(() =>
			expect(getStaticRuntimePolicyOrigins()).toEqual([
				"https://101-xp.ivanli.cc",
				fixtureCatalog.url.primaryApi(),
			]),
		);
	});
});
