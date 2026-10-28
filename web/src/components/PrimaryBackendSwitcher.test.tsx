import { describe, expect, it } from "vitest";

import type { BackendCandidate } from "@/backend/primaryBackend";

import { getCurrentReloadCandidate } from "./PrimaryBackendSwitcher";

const stagedCandidate: BackendCandidate = {
	origin: "https://node.example",
	nodeId: "node-1",
	nodeName: "Node 1",
	verifiedAt: Date.now(),
	lastError: null,
	runtimeState: "staged",
	runtimePolicyId: "rcp-current",
};

describe("getCurrentReloadCandidate", () => {
	it("rejects a confirmation captured from an older policy", () => {
		expect(
			getCurrentReloadCandidate(stagedCandidate, [
				{ ...stagedCandidate, runtimeState: "temporarily_unavailable" },
			]),
		).toBeNull();
		expect(
			getCurrentReloadCandidate(stagedCandidate, [
				{ ...stagedCandidate, runtimePolicyId: "rcp-next" },
			]),
		).toBeNull();
	});

	it("keeps the confirmation when the staged candidate is unchanged", () => {
		expect(
			getCurrentReloadCandidate(stagedCandidate, [stagedCandidate]),
		).toEqual(stagedCandidate);
	});
});
