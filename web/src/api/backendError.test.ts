import { describe, expect, it } from "vitest";

import { throwIfNotOk } from "./backendError";

function errorResponse(headers: Record<string, string>): Response {
	return new Response(
		JSON.stringify({
			error: {
				code: "peer_circuit_open",
				message: "cooldown",
				details: { retry_after_seconds: 30 },
			},
		}),
		{
			status: 503,
			headers: { "Content-Type": "application/json", ...headers },
		},
	);
}

describe("backend error Retry-After parsing", () => {
	it("preserves a bounded HTTP cooldown", async () => {
		await expect(
			throwIfNotOk(errorResponse({ "Retry-After": "45" })),
		).rejects.toMatchObject({
			retryAfterSeconds: 45,
		});
	});

	it("caps an oversized HTTP cooldown", async () => {
		await expect(
			throwIfNotOk(errorResponse({ "Retry-After": "9999" })),
		).rejects.toMatchObject({
			retryAfterSeconds: 300,
		});
	});

	it("does not infer a cooldown from malformed or missing headers", async () => {
		await expect(
			throwIfNotOk(errorResponse({ "Retry-After": "later" })),
		).rejects.toMatchObject({ retryAfterSeconds: undefined });
		await expect(throwIfNotOk(errorResponse({}))).rejects.toMatchObject({
			retryAfterSeconds: undefined,
		});
	});
});
