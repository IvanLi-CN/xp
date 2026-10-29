import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	hydratePrimaryBackendProfile: vi.fn(),
}));

vi.mock("../backend/primaryBackend", async () => {
	const actual = await vi.importActual<object>("../backend/primaryBackend");
	return {
		...actual,
		hydratePrimaryBackendProfile: mocks.hydratePrimaryBackendProfile,
	};
});

import { useHydratePrimaryBackendProfile } from "./usePublicBackendReadiness";

describe("useHydratePrimaryBackendProfile", () => {
	beforeEach(() => {
		mocks.hydratePrimaryBackendProfile.mockReset();
	});

	it("initializes static console runtime policy from public cluster identity", () => {
		renderHook(() =>
			useHydratePrimaryBackendProfile({
				clusterId: "cluster-static",
				primaryClusterId: null,
				nodesReady: false,
				staticConsole: true,
			}),
		);

		expect(mocks.hydratePrimaryBackendProfile).toHaveBeenCalledWith(
			"cluster-static",
			undefined,
		);
	});

	it("does not hydrate a mismatched cached cluster for the embedded console", () => {
		renderHook(() =>
			useHydratePrimaryBackendProfile({
				clusterId: "cluster-cached",
				primaryClusterId: "cluster-current",
				nodesReady: false,
			}),
		);

		expect(mocks.hydratePrimaryBackendProfile).not.toHaveBeenCalled();
	});
});
