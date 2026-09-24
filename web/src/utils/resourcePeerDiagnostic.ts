import { z } from "zod";

import { isBackendApiError } from "../api/backendError";

const nodeSchema = z.object({
	node_id: z.string().min(1),
	node_name: z.string().min(1),
});

const routeSchema = z.enum(["direct_mesh", "reverse_relay", "public"]);
const failureSchema = z.enum([
	"circuit_open",
	"pre_response_timeout",
	"pre_response_transport",
	"unsigned_response",
	"acknowledgement_missing",
	"acknowledgement_invalid",
	"outcome_unknown",
]);
const acknowledgementSchema = z.enum(["not_observed", "missing", "invalid"]);
const dispatchSchema = z.enum([
	"not_dispatched",
	"dispatched_no_verified_response",
]);

const routeAttemptSchema = z.object({
	route: routeSchema,
	failure: failureSchema,
	acknowledgement: acknowledgementSchema,
	dispatch: dispatchSchema,
	observed_at: z.string().min(1),
	request_id: z.string().min(1),
	elapsed_ms: z.number().int().nonnegative(),
	retry_count: z.number().int().nonnegative(),
	http_status: z.number().int().min(100).max(599).optional(),
});

const publicFailureSchema = routeAttemptSchema.omit({ route: true });

const diagnosticSchema = z.object({
	origin: nodeSchema,
	target: nodeSchema,
	route_attempts: z.array(routeAttemptSchema).max(8),
	last_public_failure: publicFailureSchema.optional(),
	public_circuit: z.enum(["closed", "open", "half_open", "disabled"]),
	request_id: z.string().min(1),
});

export type ResourcePeerDiagnostic = z.infer<typeof diagnosticSchema>;

export function parseResourcePeerDiagnostic(
	error: unknown,
): ResourcePeerDiagnostic | null {
	if (!isBackendApiError(error) || error.status !== 504) return null;
	if (error.code !== "resource_peer_unavailable") return null;
	const parsed = diagnosticSchema.safeParse(error.details?.diagnostic);
	if (!parsed.success) return null;
	if (
		!parsed.data.route_attempts.every(
			(attempt) => attempt.request_id === parsed.data.request_id,
		)
	) {
		return null;
	}
	if (
		parsed.data.last_public_failure &&
		parsed.data.last_public_failure.request_id !== parsed.data.request_id
	) {
		return null;
	}
	return parsed.data;
}
