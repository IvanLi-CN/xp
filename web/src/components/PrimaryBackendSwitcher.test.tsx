import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { BackendCandidate } from "@/backend/primaryBackend";
import type { PrimaryBackendSnapshot } from "@/backend/primaryBackend";
import { fixtureCatalog } from "@/fixture-policy/catalog";

const mockUsePrimaryBackend = vi.hoisted(() => vi.fn());

vi.mock("@/backend/PrimaryBackendProvider", () => ({
	usePrimaryBackend: mockUsePrimaryBackend,
}));

import {
	PrimaryBackendSwitcher,
	getCurrentReloadCandidate,
} from "./PrimaryBackendSwitcher";

const stagedCandidate: BackendCandidate = {
	origin: fixtureCatalog.url.secondaryApi(),
	nodeId: fixtureCatalog.identifier.nodeSecondary(),
	nodeName: fixtureCatalog.identifier.nodeNameSecondary(),
	verifiedAt: Date.now(),
	lastError: null,
	runtimeState: "staged",
	runtimePolicyId: "rcp-current",
};

const snapshot = (candidate: BackendCandidate): PrimaryBackendSnapshot => ({
	clusterId: fixtureCatalog.cluster.fixture84(),
	primaryOrigin: fixtureCatalog.url.primaryApi(),
	candidates: [candidate],
	generation: 1,
	state: "ready",
	pendingMutations: 0,
	unknownMutationCount: 0,
	lastSwitchTimedOut: false,
});

beforeEach(() => {
	mockUsePrimaryBackend.mockReturnValue(snapshot(stagedCandidate));
});

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
		expect(
			getCurrentReloadCandidate(stagedCandidate, [stagedCandidate], true),
		).toBeNull();
	});

	it("keeps the confirmation when the staged candidate is unchanged", () => {
		expect(
			getCurrentReloadCandidate(stagedCandidate, [stagedCandidate]),
		).toEqual(stagedCandidate);
	});

	it("hides a reload confirmation after the policy retires its candidate", async () => {
		const user = userEvent.setup();
		const view = render(
			<PrimaryBackendSwitcher
				adminToken="admin-token"
				clusterId={fixtureCatalog.cluster.fixture84()}
			/>,
		);

		await user.click(
			screen.getByRole("button", { name: "Open primary backend" }),
		);
		await user.click(screen.getByRole("menuitem"));
		expect(screen.getByRole("button", { name: "Reload once" })).toBeVisible();

		mockUsePrimaryBackend.mockReturnValue(
			snapshot({ ...stagedCandidate, runtimeState: "temporarily_unavailable" }),
		);
		view.rerender(
			<PrimaryBackendSwitcher
				adminToken="admin-token"
				clusterId={fixtureCatalog.cluster.fixture84()}
			/>,
		);

		expect(screen.queryByRole("button", { name: "Reload once" })).toBeNull();
	});
});
