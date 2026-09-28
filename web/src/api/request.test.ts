import { afterEach, describe, expect, it, vi } from "vitest";

import {
	ApiRequestTimeoutError,
	ApiResponseError,
	fetchJsonWithTimeout,
} from "./request";

describe("fetchJsonWithTimeout", () => {
	afterEach(() => {
		vi.restoreAllMocks();
		vi.useRealTimers();
	});

	it("parses a successful JSON response", async () => {
		vi.spyOn(globalThis, "fetch").mockResolvedValue(
			new Response(JSON.stringify({ ok: true }), {
				status: 200,
				headers: { "Content-Type": "application/json" },
			}),
		);

		await expect(
			fetchJsonWithTimeout(
				"/api/test",
				{},
				(value) => (value as { ok: boolean }).ok,
				50,
			),
		).resolves.toBe(true);
	});

	it("turns an empty response into a retryable response error", async () => {
		vi.spyOn(globalThis, "fetch").mockResolvedValue(
			new Response("", { status: 200 }),
		);

		await expect(
			fetchJsonWithTimeout("/api/test", {}, (value) => value, 50),
		).rejects.toBeInstanceOf(ApiResponseError);
	});

	it("turns a schema parsing failure into a retryable response error", async () => {
		vi.spyOn(globalThis, "fetch").mockResolvedValue(
			new Response(JSON.stringify({ ok: "not a boolean" }), { status: 200 }),
		);

		await expect(
			fetchJsonWithTimeout(
				"/api/test",
				{},
				() => {
					throw new Error("schema mismatch");
				},
				50,
			),
		).rejects.toBeInstanceOf(ApiResponseError);
	});

	it("aborts a request that never produces a response", async () => {
		vi.useFakeTimers();
		const fetchSpy = vi.spyOn(globalThis, "fetch").mockImplementation(
			(_input, init) =>
				new Promise<Response>((_resolve, reject) => {
					init?.signal?.addEventListener("abort", () => {
						reject(new DOMException("Aborted", "AbortError"));
					});
				}),
		);

		const request = fetchJsonWithTimeout("/api/test", {}, (value) => value, 25);
		const rejection = request.catch((error: unknown) => {
			expect(error).toBeInstanceOf(ApiRequestTimeoutError);
			expect(error).toMatchObject({
				name: "ApiRequestTimeoutError",
				timeoutMs: 25,
			});
		});
		await vi.advanceTimersByTimeAsync(25);

		await rejection;
		expect(fetchSpy).toHaveBeenCalledTimes(1);
	});

	it("aborts a response body that never finishes", async () => {
		vi.useFakeTimers();
		vi.spyOn(globalThis, "fetch").mockImplementation((_input, init) =>
			Promise.resolve({
				ok: true,
				json: () =>
					new Promise<never>((_resolve, reject) => {
						init?.signal?.addEventListener("abort", () => {
							reject(new DOMException("Aborted", "AbortError"));
						});
					}),
			} as unknown as Response),
		);

		const request = fetchJsonWithTimeout("/api/test", {}, (value) => value, 25);
		const rejection = request.catch((error: unknown) => {
			expect(error).toBeInstanceOf(ApiRequestTimeoutError);
		});
		await vi.advanceTimersByTimeAsync(25);

		await rejection;
	});

	it("preserves caller cancellation", async () => {
		const controller = new AbortController();
		const fetchSpy = vi.spyOn(globalThis, "fetch").mockImplementation(
			(_input, init) =>
				new Promise<Response>((_resolve, reject) => {
					init?.signal?.addEventListener("abort", () => {
						reject(new DOMException("Aborted", "AbortError"));
					});
				}),
		);

		const request = fetchJsonWithTimeout(
			"/api/test",
			{ signal: controller.signal },
			(value) => value,
			50,
		);
		controller.abort();

		await expect(request).rejects.toMatchObject({ name: "AbortError" });
		expect(fetchSpy).toHaveBeenCalledTimes(1);
	});
});
