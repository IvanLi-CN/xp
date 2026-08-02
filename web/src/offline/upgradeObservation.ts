import { isBackendApiError } from "@/api/backendError";

export const UPGRADE_OBSERVATION_INTERVAL_MS = 2_500;
export const UPGRADE_OBSERVATION_TIMEOUT_MS = 60_000;
export const UPGRADE_OBSERVATION_STORAGE_KEY = "xp.upgrade-observation";

type UpgradeJobSnapshot = {
	state: string;
	target_tag?: string | null;
};

export type UpgradeObservation = {
	targetTag: string;
	deadlineAtMs: number;
	phase: "observing" | "timed_out" | "terminal";
};

export type UpgradeStartErrorDisposition =
	| "observe"
	| "observe_existing"
	| "failed";

export function beginUpgradeObservation(
	targetTag: string,
	nowMs: number,
): UpgradeObservation {
	return {
		targetTag,
		deadlineAtMs: nowMs + UPGRADE_OBSERVATION_TIMEOUT_MS,
		phase: "observing",
	};
}

export function classifyUpgradeStartError(
	error: unknown,
): UpgradeStartErrorDisposition {
	if (!isBackendApiError(error)) return "observe";
	if (error.code === "upgrade_already_running") return "observe_existing";
	if (!error.code && error.status >= 500) return "observe";
	return "failed";
}

export function observeUpgradeStatus(
	observation: UpgradeObservation | null,
	status: UpgradeJobSnapshot | null,
	nowMs: number,
): UpgradeObservation | null {
	if (!observation) return null;
	if (observation.phase !== "observing") return observation;
	if (isTerminal(status?.state)) {
		return { ...observation, phase: "terminal" };
	}
	if (nowMs >= observation.deadlineAtMs) {
		return { ...observation, phase: "timed_out" };
	}
	if (
		isActive(status?.state) &&
		status?.target_tag &&
		status.target_tag !== observation.targetTag
	) {
		return { ...observation, targetTag: status.target_tag };
	}
	return observation;
}

export function refreshTimedOutObservation(
	observation: UpgradeObservation | null,
	status: UpgradeJobSnapshot | null,
	nowMs: number,
): UpgradeObservation | null {
	if (!observation || observation.phase !== "timed_out") return observation;
	if (isActive(status?.state)) {
		return beginUpgradeObservation(
			status?.target_tag ?? observation.targetTag,
			nowMs,
		);
	}
	if (isTerminal(status?.state)) {
		return { ...observation, phase: "terminal" };
	}
	if (status?.state === "idle") return null;
	return observation;
}

export function restoreUpgradeObservation(
	value: string | null,
	nowMs: number,
): UpgradeObservation | null {
	if (!value) return null;
	try {
		const parsed: unknown = JSON.parse(value);
		if (!isStoredObservation(parsed)) return null;
		if (parsed.phase === "timed_out" || parsed.deadlineAtMs <= nowMs) {
			return { ...parsed, phase: "timed_out" };
		}
		return parsed;
	} catch {
		return null;
	}
}

export function readUpgradeObservation(
	nowMs: number,
): UpgradeObservation | null {
	if (typeof window === "undefined") return null;
	return restoreUpgradeObservation(
		window.sessionStorage.getItem(UPGRADE_OBSERVATION_STORAGE_KEY),
		nowMs,
	);
}

export function writeUpgradeObservation(
	observation: UpgradeObservation | null,
) {
	if (typeof window === "undefined") return;
	if (observation) {
		window.sessionStorage.setItem(
			UPGRADE_OBSERVATION_STORAGE_KEY,
			JSON.stringify(observation),
		);
		return;
	}
	window.sessionStorage.removeItem(UPGRADE_OBSERVATION_STORAGE_KEY);
}

function isActive(state: string | undefined): boolean {
	return state === "running" || state === "restarting";
}

function isTerminal(state: string | undefined): boolean {
	return state === "succeeded" || state === "failed" || state === "unsupported";
}

function isStoredObservation(value: unknown): value is UpgradeObservation {
	if (!value || typeof value !== "object") return false;
	const candidate = value as Partial<UpgradeObservation>;
	return (
		typeof candidate.targetTag === "string" &&
		typeof candidate.deadlineAtMs === "number" &&
		Number.isFinite(candidate.deadlineAtMs) &&
		(candidate.phase === "observing" ||
			candidate.phase === "timed_out" ||
			candidate.phase === "terminal")
	);
}
