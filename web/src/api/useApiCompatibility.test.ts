import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { type ReactNode, createElement } from "react";
import { describe, expect, it, vi } from "vitest";

import { resolveApiCompatibility } from "./apiCompatibility";
import {
	getApiCapabilityState,
	useApiCompatibility,
} from "./useApiCompatibility";

describe("API capability state", () => {
	it("blocks protected queries while the compatibility probe is pending", () => {
		const state = getApiCapabilityState(null, "admin.nodes");

		expect(state).toMatchObject({
			available: false,
			unavailable: true,
			pending: true,
		});
		expect(state.reason).toMatch(/Checking API compatibility/);
	});

	it("degrades only the capability missing from a compatible profile", () => {
		const compatibility = resolveApiCompatibility({
			releaseTag: "v3.21.11",
			capabilities: ["api.health", "api.cluster-info", "admin.nodes"],
		});
		const state = getApiCapabilityState(compatibility, "admin.mesh");

		expect(state.available).toBe(false);
		expect(state.pending).toBe(false);
		expect(state.reason).toMatch(/admin\.mesh/);
	});

	it("does not send a token-bearing compatibility request after identity changes", async () => {
		const fetchMock = vi.fn();
		vi.stubGlobal("fetch", fetchMock);
		const queryClient = new QueryClient({
			defaultOptions: { queries: { retry: false } },
		});
		const wrapper = ({ children }: { children: ReactNode }) =>
			createElement(QueryClientProvider, { client: queryClient }, children);
		const beforeAuthenticatedRequest = vi.fn().mockResolvedValue(false);

		const { result } = renderHook(
			() =>
				useApiCompatibility(
					"unverified-token",
					true,
					beforeAuthenticatedRequest,
					"stale-cluster",
				),
			{ wrapper },
		);

		await waitFor(() => expect(result.current.isSuccess).toBe(true));
		expect(beforeAuthenticatedRequest).toHaveBeenCalledOnce();
		expect(fetchMock).not.toHaveBeenCalled();
		expect(result.current.data).toBeNull();
		queryClient.clear();
		vi.unstubAllGlobals();
	});
});
