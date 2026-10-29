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
		await user.click(
			screen.getByRole("menuitem", {
				name: stagedCandidate.nodeName,
			}),
		);
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

describe("directory state", () => {
	it("shows one stale notice while keeping nodes and does not show unknown versions", async () => {
		mockUsePrimaryBackend.mockReturnValue(
			snapshot({ ...stagedCandidate, versionState: "unknown" }),
		);
		const user = userEvent.setup();
		render(
			<PrimaryBackendSwitcher
				adminToken="admin-token"
				clusterId="cluster-test"
				directoryStatus="stale"
			/>,
		);
		await user.click(
			screen.getByRole("button", { name: "Open primary backend" }),
		);
		expect(screen.getByRole("status")).toHaveTextContent(
			"Node list may be out of date.",
		);
		expect(
			screen.getByRole("menuitem", { name: stagedCandidate.nodeName }),
		).toBeVisible();
		expect(screen.queryByText(/version unknown/i)).toBeNull();
	});

	it("shows a failure instead of rows before the first inventory", async () => {
		const user = userEvent.setup();
		render(
			<PrimaryBackendSwitcher
				adminToken="admin-token"
				clusterId="cluster-test"
				directoryStatus="error"
			/>,
		);
		await user.click(
			screen.getByRole("button", { name: "Open primary backend" }),
		);
		expect(screen.getByRole("alert")).toHaveTextContent(
			"Unable to load node list.",
		);
		expect(
			screen.queryByRole("menuitem", { name: stagedCandidate.nodeName }),
		).toBeNull();
	});
});
