import { afterEach, describe, expect, it, vi } from "vitest";

import { fixtureCatalog } from "../fixture-policy/catalog";
import {
	fetchAdminMembershipOperation,
	fetchAdminNodeDeletePreview,
} from "./adminNodes";
import { ApiRequestTimeoutError } from "./request";

describe("fetchAdminNodeDeletePreview", () => {
	afterEach(() => {
		vi.restoreAllMocks();
		vi.useRealTimers();
	});

	it("uses the bounded read request helper", async () => {
		const nodeId = fixtureCatalog.nodeId.fixture134();
		const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue(
			new Response(
				JSON.stringify({
					node_id: nodeId,
					endpoints: [],
				}),
				{ status: 200, headers: { "Content-Type": "application/json" } },
			),
		);

		await expect(
			fetchAdminNodeDeletePreview("admintoken", nodeId),
		).resolves.toEqual({ node_id: nodeId, endpoints: [] });
		expect(fetchSpy).toHaveBeenCalledWith(
			`/api/admin/nodes/${encodeURIComponent(nodeId)}/delete-preview`,
			expect.objectContaining({
				method: "GET",
				signal: expect.any(AbortSignal),
			}),
		);
	});

	it("stops waiting when the preview request times out", async () => {
		vi.useFakeTimers();
		vi.spyOn(globalThis, "fetch").mockImplementation(
			(_input, init) =>
				new Promise<Response>((_resolve, reject) => {
					init?.signal?.addEventListener("abort", () => {
						reject(new DOMException("Aborted", "AbortError"));
					});
				}),
		);

		const request = fetchAdminNodeDeletePreview(
			"admintoken",
			fixtureCatalog.nodeId.fixture134(),
		);
		const rejection = request.catch((error: unknown) => error);
		await vi.advanceTimersByTimeAsync(8_000);

		await expect(rejection).resolves.toBeInstanceOf(ApiRequestTimeoutError);
	});

	it("bounds membership operation status reads", async () => {
		vi.useFakeTimers();
		vi.spyOn(globalThis, "fetch").mockImplementation(
			(_input, init) =>
				new Promise<Response>((_resolve, reject) => {
					init?.signal?.addEventListener("abort", () => {
						reject(new DOMException("Aborted", "AbortError"));
					});
				}),
		);

		const request = fetchAdminMembershipOperation("admintoken", "operation-1");
		const rejection = request.catch((error: unknown) => error);
		await vi.advanceTimersByTimeAsync(8_000);

		await expect(rejection).resolves.toBeInstanceOf(ApiRequestTimeoutError);
	});
});
