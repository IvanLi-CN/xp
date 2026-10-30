import { RELEASE_INVENTORIES } from "@/api/releaseInventories";

export type StaticBackendRuntimeState =
	| "active"
	| "staged"
	| "temporarily_unavailable";

export type StaticRuntimePolicyGrant = {
	policy_id: string;
	cluster_id: string;
	expires_at: string;
	api_origins: string[];
};

export type BackendCandidate = {
	origin: string;
	nodeId: string;
	nodeName: string;
	verifiedAt: number | null;
	lastError: string | null;
	switchable?: boolean;
	switchBlockedReason?: string;
	serviceVersion?: string | null;
	versionState?: "ready" | "unknown" | "unavailable";
	runtimeState?: StaticBackendRuntimeState;
	runtimePolicyId?: string;
	runtimePolicyGrant?: StaticRuntimePolicyGrant;
};

export type BackendProfile = {
	clusterId: string;
	primaryOrigin: string;
	candidates: BackendCandidate[];
	updatedAt: number;
};

export type PrimaryBackendSnapshot = {
	clusterId: string | null;
	primaryOrigin: string;
	candidates: BackendCandidate[];
	generation: number;
	state: "ready" | "switching" | "unreachable";
	pendingMutations: number;
	unknownMutationCount: number;
	lastSwitchTimedOut: boolean;
};

export type SwitchResult = { timedOut: boolean };
export type SwitchOptions = {
	revalidate?: (candidate: BackendCandidate) => Promise<boolean>;
};

export const STATIC_WEB_ORIGIN = "https://xp.ivanli.cc";
export const STATIC_BOOTSTRAP_ORIGIN = "https://101-xp.ivanli.cc";

export function getPageOrigin(): string {
	return typeof window === "undefined" ? "" : window.location.origin;
}

export function isApiUrl(url: URL): boolean {
	return url.pathname === "/api" || url.pathname.startsWith("/api/");
}

export function isAbortError(error: unknown): boolean {
	return (
		(error instanceof DOMException || error instanceof Error) &&
		error.name === "AbortError"
	);
}

export function displayNodeName(nodeName: string, origin: string): string {
	if (nodeName.trim() && nodeName !== "Verified backend") return nodeName;
	try {
		return new URL(origin).host;
	} catch {
		return origin;
	}
}

export function currentWebReleaseVersion(): string | null {
	return recognizedReleaseVersion(RELEASE_INVENTORIES[0]?.releaseTag);
}

export function normalizeReleaseVersion(value: string): string {
	return value.trim().replace(/^v/i, "");
}

export function recognizedReleaseVersion(
	value: string | null | undefined,
): string | null {
	if (!value) return null;
	const normalized = normalizeReleaseVersion(value);
	return /^\d+\.\d+\.\d+$/.test(normalized) ? normalized : null;
}

export function canonicalBackendOrigin(value: string): string | null {
	try {
		const url = new URL(value);
		if (
			(url.protocol !== "https:" && url.origin !== getPageOrigin()) ||
			(url.pathname !== "/" && url.pathname !== "") ||
			url.search ||
			url.hash ||
			url.username ||
			url.password ||
			url.href.includes("@")
		)
			return null;
		return url.origin;
	} catch {
		return null;
	}
}

export function rewriteRequest(
	request: Request,
	primaryOrigin: string,
): Request {
	const requestUrl = new URL(request.url, getPageOrigin());
	if (!isApiUrl(requestUrl) || requestUrl.origin !== getPageOrigin()) {
		return request;
	}
	if (!primaryOrigin || primaryOrigin === getPageOrigin()) {
		return request;
	}
	const target = new URL(
		requestUrl.pathname + requestUrl.search,
		primaryOrigin,
	);
	return new Request(target, request);
}
