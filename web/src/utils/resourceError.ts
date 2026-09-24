import { type BackendApiError, isBackendApiError } from "../api/backendError";

export type ResourceFailureLayer =
	| "frontend_request"
	| "xp_api"
	| "peer_transport"
	| "peer_protocol"
	| "circuit_breaker"
	| "remote_node"
	| "unknown";

export type ResourceDiagnostic = {
	layer: ResourceFailureLayer;
	title: string;
	description: string;
	code: string;
	status?: number;
	cause?: string;
	confidence: string;
	attemptedPath: string;
	dispatchState: string;
	retryable: boolean;
	retryAfterSeconds?: number;
	targetStatus?: number;
	supportId?: string;
};

const SAFE_CAUSES = new Set([
	"offline",
	"circuit_open",
	"peer_timeout",
	"transport_error",
	"outcome_unknown",
	"protocol_rejected",
	"peer_authentication_failed",
	"invalid_target",
	"route_unavailable",
	"remote_resource_error",
	"api_error",
]);

function detailString(
	details: Record<string, unknown>,
	key: string,
): string | undefined {
	const value = details[key];
	return typeof value === "string" && value.length <= 80 ? value : undefined;
}

function detailBoolean(
	details: Record<string, unknown>,
	key: string,
): boolean | undefined {
	const value = details[key];
	return typeof value === "boolean" ? value : undefined;
}

function detailNumber(
	details: Record<string, unknown>,
	key: string,
): number | undefined {
	const value = details[key];
	return typeof value === "number" && Number.isFinite(value)
		? value
		: undefined;
}

function boundedRetryAfter(
	details: Record<string, unknown>,
	error: BackendApiError | undefined,
): number | undefined {
	const value =
		error?.retryAfterSeconds ?? detailNumber(details, "retry_after_seconds");
	return value !== undefined && Number.isInteger(value) && value > 0
		? Math.min(value, 300)
		: undefined;
}

function copyForLayer(
	layer: ResourceFailureLayer,
): Pick<ResourceDiagnostic, "title" | "description"> {
	switch (layer) {
		case "frontend_request":
			return {
				title: "Resource reads paused",
				description:
					"The browser is offline. Existing resource data remains " +
					"available until the connection returns.",
			};
		case "xp_api":
			return {
				title: "Unable to read resources from the XP API",
				description:
					"The local API did not provide a usable resource response. No node action was taken.",
			};
		case "peer_transport":
			return {
				title: "Could not obtain a verified resource response",
				description:
					"XP attempted the displayed peer path, but the result could not " +
					"be verified. Resource reads are paused.",
			};
		case "peer_protocol":
			return {
				title: "Peer response could not be verified",
				description:
					"The peer path responded, but its signed acknowledgement was not " +
					"accepted. No remote data was used.",
			};
		case "circuit_breaker":
			return {
				title: "Peer path is cooling down",
				description:
					"No new resource request was sent while the circuit is open. Retry after the cooldown ends.",
			};
		case "remote_node":
			return {
				title: "Target node returned a resource error",
				description:
					"The target node returned a verified application error. This does " +
					"not by itself indicate that the node is down.",
			};
		default:
			return {
				title: "Resource read failed",
				description:
					"There is not enough verified information to identify the cause. Resource reads are paused.",
			};
	}
}

function inferLayer(
	error: unknown,
	details: Record<string, unknown>,
	isOnline: boolean,
): ResourceFailureLayer {
	if (!isOnline) return "frontend_request";
	const declared = detailString(details, "failure_layer");
	if (
		declared === "frontend_request" ||
		declared === "xp_api" ||
		declared === "peer_transport" ||
		declared === "peer_protocol" ||
		declared === "circuit_breaker" ||
		declared === "remote_node"
	) {
		return declared;
	}
	if (isBackendApiError(error)) {
		if (error.code === "peer_circuit_open") return "circuit_breaker";
		if (error.code === "peer_protocol_rejected") return "peer_protocol";
		if (
			error.code === "peer_transport_timeout" ||
			error.code === "peer_transport_unknown" ||
			error.code === "peer_target_invalid" ||
			error.code === "peer_route_unavailable"
		) {
			return "peer_transport";
		}
		if (error.code === "remote_node_error") return "remote_node";
		if (error.status >= 400 && error.status < 500) return "xp_api";
	}
	return "unknown";
}

export function classifyResourceError(
	error: unknown,
	options: { isOnline: boolean },
): ResourceDiagnostic {
	const backendError = isBackendApiError(error) ? error : undefined;
	const details = backendError?.details ?? {};
	const layer = inferLayer(error, details, options.isOnline);
	const copy = copyForLayer(layer);
	const code =
		backendError?.code ??
		(layer === "frontend_request" ? "offline" : "unknown");
	const cause = detailString(details, "cause");
	return {
		...copy,
		layer,
		code,
		status: backendError?.status,
		cause: cause && SAFE_CAUSES.has(cause) ? cause : undefined,
		confidence: detailString(details, "confidence") ?? "unknown",
		attemptedPath: detailString(details, "attempted_path") ?? "unknown",
		dispatchState: detailString(details, "dispatch_state") ?? "unknown",
		retryable:
			detailBoolean(details, "retryable") ?? layer !== "frontend_request",
		retryAfterSeconds: boundedRetryAfter(details, backendError),
		targetStatus: detailNumber(details, "target_status"),
		supportId: detailString(details, "support_id"),
	};
}
