import { describe, expect, it } from "vitest";

import { BackendApiError } from "../api/backendError";
import { classifyResourceError } from "./resourceError";

describe("classifyResourceError", () => {
	it("keeps circuit cooldown details without exposing transport text", () => {
		const diagnostic = classifyResourceError(
			new BackendApiError({
				status: 503,
				code: "peer_circuit_open",
				message: "private internal transport detail",
				details: {
					failure_layer: "circuit_breaker",
					cause: "circuit_open",
					confidence: "confirmed",
					dispatch_state: "not_dispatched",
					retryable: true,
					retry_after_seconds: 7,
					support_id: "01JRESOURCEFAILURE",
				},
			}),
			{ isOnline: true },
		);

		expect(diagnostic.layer).toBe("circuit_breaker");
		expect(diagnostic.retryAfterSeconds).toBe(7);
		expect(diagnostic.dispatchState).toBe("not_dispatched");
		expect(diagnostic.title).toBe("Peer path is cooling down");
		expect(diagnostic.description).not.toContain("private internal");
	});

	it("preserves verified remote status as a remote-node failure", () => {
		const diagnostic = classifyResourceError(
			new BackendApiError({
				status: 429,
				code: "remote_node_error",
				message: "remote body should not become UI copy",
				details: {
					failure_layer: "remote_node",
					target_status: 429,
					retryable: true,
				},
			}),
			{ isOnline: true },
		);

		expect(diagnostic.layer).toBe("remote_node");
		expect(diagnostic.status).toBe(429);
		expect(diagnostic.targetStatus).toBe(429);
		expect(diagnostic.title).toBe("Target node returned a resource error");
		expect(diagnostic.description).not.toContain("remote body");
	});

	it("uses an unknown fallback for unstructured online failures", () => {
		const diagnostic = classifyResourceError(
			new Error("raw mesh error with a secret-looking URL"),
			{ isOnline: true },
		);

		expect(diagnostic.layer).toBe("unknown");
		expect(diagnostic.code).toBe("unknown");
		expect(diagnostic.title).toBe("Resource read failed");
		expect(diagnostic.description).not.toContain("secret-looking");
	});

	it("classifies offline state before interpreting the last error", () => {
		const diagnostic = classifyResourceError(
			new BackendApiError({
				status: 504,
				code: "peer_transport_timeout",
				message: "timeout",
			}),
			{ isOnline: false },
		);

		expect(diagnostic.layer).toBe("frontend_request");
		expect(diagnostic.title).toBe("Resource reads paused");
		expect(diagnostic.retryable).toBe(false);
	});

	it("treats unsupported monitoring as a confirmed non-retryable state", () => {
		const diagnostic = classifyResourceError(
			new BackendApiError({
				status: 501,
				code: "resource_monitoring_unsupported",
				message: "unsupported",
				details: {
					failure_layer: "unknown",
					cause: "capability_unsupported",
					retryable: false,
				},
			}),
			{ isOnline: true },
		);

		expect(diagnostic.layer).toBe("unsupported");
		expect(diagnostic.retryable).toBe(false);
		expect(diagnostic.title).toBe("Resource monitoring is unavailable");
	});

	it("falls back to safe values for untrusted diagnostic fields", () => {
		const diagnostic = classifyResourceError(
			new BackendApiError({
				status: 502,
				code: "peer_protocol_rejected",
				message: "hidden",
				details: {
					failure_layer: "peer_protocol",
					cause: "https://secret.invalid",
					confidence: "guess",
					attempted_path: "https://10.0.0.1",
					dispatch_state: "stack trace",
					support_id: "not an opaque id",
				},
			}),
			{ isOnline: true },
		);

		expect(diagnostic.cause).toBeUndefined();
		expect(diagnostic.confidence).toBe("unknown");
		expect(diagnostic.attemptedPath).toBe("unknown");
		expect(diagnostic.dispatchState).toBe("unknown");
		expect(diagnostic.supportId).toBeUndefined();
	});
});
