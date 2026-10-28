import type { AdminUserMihomoProfile } from "../api/adminUsers";
import { isBackendApiError } from "../api/backendError";

export type MihomoSaveVerification = "matches" | "different" | "unavailable";

export type MihomoSaveDiagnostics = {
	code?: string;
	cause?: string;
	dispatchState?: string;
	supportId?: string;
	targetNodeId?: string;
};

export type MihomoSaveFeedback = {
	kind: "outcome_unknown";
	verification: MihomoSaveVerification;
	diagnostics: MihomoSaveDiagnostics;
};

export class MihomoSaveOutcomeUnknownError extends Error {
	readonly submittedProfile: AdminUserMihomoProfile;
	readonly originalError: unknown;

	constructor(
		originalError: unknown,
		submittedProfile: AdminUserMihomoProfile,
	) {
		super(
			"Save result is unknown. The change may already be applied; " +
				"review the read-back before retrying.",
		);
		this.name = "MihomoSaveOutcomeUnknownError";
		this.originalError = originalError;
		this.submittedProfile = { ...submittedProfile };
	}
}

export function mihomoProfilesEqual(
	left: AdminUserMihomoProfile,
	right: AdminUserMihomoProfile,
): boolean {
	return (
		left.mixin_yaml === right.mixin_yaml &&
		left.extra_proxies_yaml === right.extra_proxies_yaml &&
		left.extra_proxy_providers_yaml === right.extra_proxy_providers_yaml
	);
}

function sourceError(error: unknown): unknown {
	return error instanceof MihomoSaveOutcomeUnknownError
		? error.originalError
		: error;
}

export function submittedMihomoProfile(
	error: unknown,
): AdminUserMihomoProfile | undefined {
	return error instanceof MihomoSaveOutcomeUnknownError
		? { ...error.submittedProfile }
		: undefined;
}

export function isMihomoWriteOutcomeUnknown(error: unknown): boolean {
	if (error instanceof MihomoSaveOutcomeUnknownError) return true;
	if (isBackendApiError(error)) {
		return error.status >= 500 && error.code !== "raft_write_not_dispatched";
	}
	if (error instanceof TypeError) return true;
	return (
		typeof DOMException !== "undefined" &&
		error instanceof DOMException &&
		error.name === "AbortError"
	);
}

export function mihomoSaveDiagnostics(error: unknown): MihomoSaveDiagnostics {
	const source = sourceError(error);
	if (!isBackendApiError(source)) return {};
	const details = source.details ?? {};
	const stringDetail = (key: string): string | undefined => {
		const value = details[key];
		return typeof value === "string" && value.length > 0 ? value : undefined;
	};
	return {
		code: source.code,
		cause: stringDetail("cause"),
		dispatchState: stringDetail("dispatch_state"),
		supportId: stringDetail("support_id"),
		targetNodeId: stringDetail("target_node_id"),
	};
}

export function formatMihomoSaveError(error: unknown): string {
	const source = sourceError(error);
	if (isBackendApiError(source)) {
		if (source.code === "raft_write_not_dispatched") {
			return (
				"Save was not sent to the leader. Your draft is still here; " +
				"retry when the cluster is ready."
			);
		}
		if (source.status >= 500 || source.code === "raft_write_outcome_unknown") {
			return (
				"Save result is unknown. It may already be applied; " +
				"check the saved profile before retrying."
			);
		}
	}
	if (source instanceof TypeError) {
		return (
			"Save result is unknown. The browser lost the response; " +
			"the change may already be applied."
		);
	}
	if (
		typeof DOMException !== "undefined" &&
		source instanceof DOMException &&
		source.name === "AbortError"
	) {
		return "Save result is unknown. The request was interrupted; the change may already be applied.";
	}
	if (error instanceof Error) return error.message;
	return String(error);
}
