import { describe, expect, it } from "vitest";

import { BackendApiError } from "../api/backendError";
import { parseResourcePeerDiagnostic } from "./resourcePeerDiagnostic";

const failures = [
	"circuit_open",
	"pre_response_timeout",
	"pre_response_transport",
	"unsigned_response",
	"acknowledgement_missing",
	"acknowledgement_invalid",
	"outcome_unknown",
] as const;

function errorFor(failure: (typeof failures)[number]) {
	return new BackendApiError({
		status: 504,
		code: "resource_peer_unavailable",
		message: "resource snapshot is unavailable from the target node",
		details: {
			diagnostic: {
				origin: { node_id: "101", node_name: "101" },
				target: { node_id: "us", node_name: "us" },
				route_attempts: [
					{
						route: "direct_mesh",
						failure,
						acknowledgement: "not_observed",
						dispatch: "dispatched_no_verified_response",
						observed_at: "2026-09-24T00:00:00.000Z",
						request_id: "01M0C1SJ5M1JWE6CCKMXNXPZ78",
						elapsed_ms: 100,
						retry_count: 0,
					},
				],
				public_circuit: "closed",
				request_id: "01M0C1SJ5M1JWE6CCKMXNXPZ78",
			},
		},
	});
}

describe("parseResourcePeerDiagnostic", () => {
	it.each(failures)("accepts the stable %s failure category", (failure) => {
		expect(
			parseResourcePeerDiagnostic(errorFor(failure))?.route_attempts[0],
		).toMatchObject({
			failure,
		});
	});

	it("strips unapproved transport details", () => {
		const error = errorFor("pre_response_transport");
		const diagnostic = error.details?.diagnostic as Record<string, unknown>;
		(diagnostic.route_attempts as Array<Record<string, unknown>>)[0].url =
			"https://secret.example/internal";
		(
			diagnostic.route_attempts as Array<Record<string, unknown>>
		)[0].transport_error = "connection reset";

		const parsed = parseResourcePeerDiagnostic(error);
		expect(parsed?.route_attempts[0]).not.toHaveProperty("url");
		expect(parsed?.route_attempts[0]).not.toHaveProperty("transport_error");
	});

	it("rejects an unstructured or unrelated timeout", () => {
		expect(
			parseResourcePeerDiagnostic(
				new BackendApiError({
					status: 504,
					code: "timeout",
					message: "timeout",
				}),
			),
		).toBeNull();
	});

	it("rejects route records with a different correlation id", () => {
		const error = errorFor("pre_response_timeout");
		const diagnostic = error.details?.diagnostic as Record<string, unknown>;
		(
			diagnostic.route_attempts as Array<Record<string, unknown>>
		)[0].request_id = "01M0C1SJ5M1JWE6CCKMXNXPZ79";

		expect(parseResourcePeerDiagnostic(error)).toBeNull();
	});
});
