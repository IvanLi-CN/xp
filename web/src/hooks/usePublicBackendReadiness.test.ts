import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	getPrimaryBackendSnapshot: vi.fn(),
	hydratePrimaryBackendProfile: vi.fn(),
	probeBackendOrigin: vi.fn(),
}));

vi.mock("../backend/primaryBackend", async () => {
	const actual = await vi.importActual<object>("../backend/primaryBackend");
	return {
		...actual,
		getPrimaryBackendSnapshot: mocks.getPrimaryBackendSnapshot,
		hydratePrimaryBackendProfile: mocks.hydratePrimaryBackendProfile,
		probeBackendOrigin: mocks.probeBackendOrigin,
	};
});

import {
	useAuthenticatedBackendProbe,
	useHydratePrimaryBackendProfile,
} from "./usePublicBackendReadiness";

describe("useHydratePrimaryBackendProfile", () => {
	beforeEach(() => {
		mocks.getPrimaryBackendSnapshot.mockReset();
		mocks.hydratePrimaryBackendProfile.mockReset();
		mocks.probeBackendOrigin.mockReset();
	});

	it("rechecks public identity when returning from offline mode", async () => {
		mocks.probeBackendOrigin.mockResolvedValue({
			clusterId: "cluster-current",
		});
		mocks.getPrimaryBackendSnapshot.mockReturnValue({
			primaryOrigin: "https://node.example",
			clusterId: "cluster-current",
		});
		const { result, rerender } = renderHook(
			({ isOnline }: { isOnline: boolean }) =>
				useAuthenticatedBackendProbe({
					staticConsole: false,
					isOnline,
					origin: "https://node.example",
					clusterId: "cluster-current",
				}),
			{ initialProps: { isOnline: false } },
		);

		expect(result.current.ready).toBe(true);
		rerender({ isOnline: true });
		expect(result.current.ready).toBe(false);
		await waitFor(() => expect(result.current.ready).toBe(true));
		expect(mocks.probeBackendOrigin).toHaveBeenCalledWith({
			origin: "https://node.example",
			signal: expect.any(AbortSignal),
		});
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