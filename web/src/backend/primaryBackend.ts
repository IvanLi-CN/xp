import { resolveApiCompatibility } from "@/api/apiCompatibility";

export const PRIMARY_BACKEND_SWITCH_TIMEOUT_MS = 60_000;

export type BackendCandidate = {
	origin: string;
	nodeId: string;
	nodeName: string;
	verifiedAt: number | null;
	lastError: string | null;
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

export type SwitchResult = {
	timedOut: boolean;
};

type StoredProfiles = {
	activeClusterId?: string;
	profiles?: Record<string, BackendProfile>;
};

type NativeFetch = typeof window.fetch;

const STORAGE_KEY = "xp-primary-backend-profiles";
const MUTATING_METHODS = new Set(["POST", "PUT", "PATCH", "DELETE"]);
const listeners = new Set<(snapshot: PrimaryBackendSnapshot) => void>();
const mutationWaiters = new Set<(timedOut: boolean) => void>();

let nativeFetch: NativeFetch | null = null;
let installed = false;

function getPageOrigin(): string {
	return typeof window === "undefined" ? "" : window.location.origin;
}

function readStoredProfiles(): StoredProfiles {
	if (typeof window === "undefined") return {};
	try {
		const value = window.localStorage.getItem(STORAGE_KEY);
		if (!value) return {};
		const parsed = JSON.parse(value) as StoredProfiles;
		return parsed && typeof parsed === "object" ? parsed : {};
	} catch {
		return {};
	}
}

function readInitialState(): PrimaryBackendSnapshot {
	const stored = readStoredProfiles();
	const active = stored.activeClusterId
		? stored.profiles?.[stored.activeClusterId]
		: undefined;
	return {
		clusterId: stored.activeClusterId ?? null,
		primaryOrigin: getPageOrigin(),
		candidates: active?.candidates ?? [],
		generation: 0,
		state: "ready",
		pendingMutations: 0,
		unknownMutationCount: 0,
		lastSwitchTimedOut: false,
	};
}

let state: PrimaryBackendSnapshot = readInitialState();

function writeStoredProfiles(value: StoredProfiles) {
	if (typeof window === "undefined") return;
	try {
		window.localStorage.setItem(STORAGE_KEY, JSON.stringify(value));
	} catch {
		// Private browsing and quota errors must not make the console unusable.
	}
}

function isApiUrl(url: URL): boolean {
	return url.pathname === "/api" || url.pathname.startsWith("/api/");
}

function isAbortError(error: unknown): boolean {
	return (
		(error instanceof DOMException && error.name === "AbortError") ||
		(error instanceof Error && error.name === "AbortError")
	);
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
			url.password
		) {
			return null;
		}
		return url.origin;
	} catch {
		return null;
	}
}

function notify() {
	const snapshot = getPrimaryBackendSnapshot();
	for (const listener of listeners) listener(snapshot);
}

function persistCurrentProfile() {
	if (!state.clusterId) return;
	const stored = readStoredProfiles();
	const profile: BackendProfile = {
		clusterId: state.clusterId,
		primaryOrigin: state.primaryOrigin,
		candidates: state.candidates,
		updatedAt: Date.now(),
	};
	writeStoredProfiles({
		activeClusterId: state.clusterId,
		profiles: { ...(stored.profiles ?? {}), [state.clusterId]: profile },
	});
}

function updateState(patch: Partial<PrimaryBackendSnapshot>) {
	state = { ...state, ...patch };
	notify();
}

function markRequestResult(response: Response | null, error?: unknown) {
	if (
		(error && !isAbortError(error)) ||
		(response !== null && response.status >= 500)
	) {
		if (state.state !== "switching") updateState({ state: "unreachable" });
		return;
	}
	if (response?.ok && state.state === "unreachable") {
		updateState({ state: "ready" });
	}
}

function rewriteRequest(request: Request): Request {
	const requestUrl = new URL(request.url, getPageOrigin());
	if (!isApiUrl(requestUrl) || requestUrl.origin !== getPageOrigin()) {
		return request;
	}
	if (!state.primaryOrigin || state.primaryOrigin === getPageOrigin()) {
		return request;
	}
	const target = new URL(
		requestUrl.pathname + requestUrl.search,
		state.primaryOrigin,
	);
	return new Request(target, request);
}

function beginMutation() {
	state = {
		...state,
		pendingMutations: state.pendingMutations + 1,
		lastSwitchTimedOut: false,
	};
	notify();
	let released = false;
	return () => {
		if (released) return;
		released = true;
		state = {
			...state,
			pendingMutations: Math.max(0, state.pendingMutations - 1),
		};
		if (state.pendingMutations === 0) {
			for (const waiter of mutationWaiters) waiter(false);
			mutationWaiters.clear();
		}
		notify();
	};
}

export function installPrimaryBackendTransport() {
	if (installed || typeof window === "undefined") return;
	installed = true;
	nativeFetch = window.fetch.bind(window);
	window.fetch = async (input, init) => {
		const normalizedInput =
			typeof input === "string"
				? new URL(input, getPageOrigin()).toString()
				: input;
		const request = new Request(normalizedInput, init);
		const isApiRequest = isApiUrl(new URL(request.url, getPageOrigin()));
		const isMutation =
			isApiRequest && MUTATING_METHODS.has(request.method.toUpperCase());
		const release = isMutation ? beginMutation() : null;
		try {
			const response = await nativeFetch?.(rewriteRequest(request));
			if (!response) throw new Error("fetch is unavailable");
			if (isApiRequest) markRequestResult(response);
			return response;
		} catch (error) {
			if (isApiRequest) markRequestResult(null, error);
			throw error;
		} finally {
			release?.();
		}
	};
}

export function resetPrimaryBackendTransportForTests() {
	if (installed && nativeFetch && typeof window !== "undefined") {
		window.fetch = nativeFetch;
	}
	installed = false;
	nativeFetch = null;
	listeners.clear();
	mutationWaiters.clear();
	state = readInitialState();
}

export function getPrimaryBackendSnapshot(): PrimaryBackendSnapshot {
	return {
		...state,
		candidates: state.candidates.map((candidate) => ({ ...candidate })),
	};
}

export function subscribePrimaryBackend(
	listener: (snapshot: PrimaryBackendSnapshot) => void,
) {
	listeners.add(listener);
	listener(getPrimaryBackendSnapshot());
	return () => {
		listeners.delete(listener);
	};
}

export function hydratePrimaryBackendProfile(
	clusterId: string,
	nodes?: Array<{
		node_id: string;
		node_name: string;
		api_base_url: string;
	}>,
) {
	const stored = readStoredProfiles();
	const storedProfile = stored.profiles?.[clusterId];
	const candidatesByOrigin = new Map<string, BackendCandidate>();
	if (nodes === undefined) {
		for (const candidate of storedProfile?.candidates ?? []) {
			if (canonicalBackendOrigin(candidate.origin)) {
				candidatesByOrigin.set(candidate.origin, { ...candidate });
			}
		}
	}
	for (const node of nodes ?? []) {
		const origin = canonicalBackendOrigin(node.api_base_url);
		if (!origin) continue;
		const existing = candidatesByOrigin.get(origin);
		candidatesByOrigin.set(origin, {
			origin,
			nodeId: node.node_id,
			nodeName: node.node_name,
			verifiedAt: existing?.verifiedAt ?? null,
			lastError: existing?.lastError ?? null,
		});
	}
	const pageOrigin = getPageOrigin();
	if (pageOrigin) {
		candidatesByOrigin.set(pageOrigin, {
			origin: pageOrigin,
			nodeId: "current-page",
			nodeName: "Current page",
			verifiedAt: Date.now(),
			lastError: null,
		});
	}
	const persistedPrimary = canonicalBackendOrigin(
		storedProfile?.primaryOrigin ?? "",
	);
	const nextPrimary =
		persistedPrimary &&
		(nodes === undefined || candidatesByOrigin.has(persistedPrimary))
			? persistedPrimary
			: pageOrigin;
	const changed =
		state.clusterId !== clusterId || state.primaryOrigin !== nextPrimary;
	state = {
		...state,
		clusterId,
		primaryOrigin: nextPrimary,
		candidates: [...candidatesByOrigin.values()],
		generation: changed ? state.generation + 1 : state.generation,
		state: "ready",
	};
	persistCurrentProfile();
	notify();
}

function directFetch(input: RequestInfo | URL, init?: RequestInit) {
	const fetcher =
		nativeFetch ??
		(typeof window === "undefined" ? null : window.fetch.bind(window));
	if (!fetcher) throw new Error("fetch is unavailable");
	return fetcher(input, init);
}

async function directJson(
	origin: string,
	path: string,
	token?: string,
	signal?: AbortSignal,
) {
	const response = await directFetch(new URL(path, origin), {
		method: "GET",
		headers: {
			Accept: "application/json",
			...(token ? { Authorization: `Bearer ${token}` } : {}),
		},
		signal,
	});
	if (!response.ok) {
		const error = new Error(`backend request failed: ${response.status}`);
		Object.assign(error, { status: response.status });
		throw error;
	}
	return (await response.json()) as unknown;
}

export async function verifyBackendCandidate(args: {
	origin: string;
	clusterId: string;
	adminToken: string;
	signal?: AbortSignal;
}): Promise<BackendCandidate> {
	const origin = canonicalBackendOrigin(args.origin);
	if (!origin) throw new Error("Backend origin must be an HTTPS origin.");
	const health = await directJson(
		origin,
		"/api/health",
		undefined,
		args.signal,
	);
	if (
		!health ||
		typeof health !== "object" ||
		(health as { status?: string }).status !== "ok"
	) {
		throw new Error("Backend health check failed.");
	}
	const cluster = await directJson(
		origin,
		"/api/cluster/info",
		undefined,
		args.signal,
	);
	if (
		!cluster ||
		typeof cluster !== "object" ||
		(cluster as { cluster_id?: string }).cluster_id !== args.clusterId
	) {
		throw new Error("Backend belongs to a different cluster.");
	}

	let compatibilityPayload: Record<string, unknown>;
	try {
		compatibilityPayload = (await directJson(
			origin,
			"/api/capabilities",
			undefined,
			args.signal,
		)) as Record<string, unknown>;
	} catch (error) {
		if ((error as { status?: number }).status !== 404) throw error;
		compatibilityPayload = (await directJson(
			origin,
			"/api/version/check",
			undefined,
			args.signal,
		)) as Record<string, unknown>;
	}
	const compatibility = resolveApiCompatibility({
		releaseTag:
			typeof compatibilityPayload.release_tag === "string"
				? compatibilityPayload.release_tag
				: typeof (
							compatibilityPayload.current as
								| { release_tag?: unknown }
								| undefined
						)?.release_tag === "string"
					? (compatibilityPayload.current as { release_tag: string })
							.release_tag
					: undefined,
		capabilities: Array.isArray(compatibilityPayload.capabilities)
			? compatibilityPayload.capabilities.filter(
					(value): value is string => typeof value === "string",
				)
			: undefined,
		fingerprint:
			typeof compatibilityPayload.fingerprint === "object" &&
			compatibilityPayload.fingerprint !== null
				? (compatibilityPayload.fingerprint as Record<string, string[]>)
				: undefined,
	});
	if (compatibility.kind === "incompatible") {
		throw new Error(compatibility.reason);
	}
	const nodes = await directJson(
		origin,
		"/api/admin/nodes",
		args.adminToken,
		args.signal,
	);
	if (
		!nodes ||
		typeof nodes !== "object" ||
		!Array.isArray((nodes as { items?: unknown }).items)
	) {
		throw new Error("Backend returned an invalid node inventory.");
	}
	const node = (
		nodes as {
			items: Array<{
				node_id?: unknown;
				node_name?: unknown;
				api_base_url?: unknown;
			}>;
		}
	).items.find(
		(candidate) =>
			candidate.api_base_url === args.origin ||
			canonicalBackendOrigin(String(candidate.api_base_url ?? "")) === origin,
	);
	if (!node && origin !== getPageOrigin()) {
		throw new Error("Backend is not a registered cluster node.");
	}
	return {
		origin,
		nodeId: typeof node?.node_id === "string" ? node.node_id : origin,
		nodeName: typeof node?.node_name === "string" ? node.node_name : origin,
		verifiedAt: Date.now(),
		lastError: null,
	};
}

async function waitForMutations(): Promise<boolean> {
	if (state.pendingMutations === 0) return false;
	return new Promise((resolve) => {
		let settled = false;
		const settle = (timedOut: boolean) => {
			if (settled) return;
			settled = true;
			window.clearTimeout(timeoutId);
			mutationWaiters.delete(settle);
			resolve(timedOut);
		};
		const timeoutId = window.setTimeout(
			() => settle(true),
			PRIMARY_BACKEND_SWITCH_TIMEOUT_MS,
		);
		mutationWaiters.add(settle);
	});
}

export async function switchPrimaryBackend(
	candidate: BackendCandidate,
): Promise<SwitchResult> {
	const origin = canonicalBackendOrigin(candidate.origin);
	if (!origin) throw new Error("Backend origin must be an HTTPS origin.");
	if (!candidate.verifiedAt) {
		throw new Error("Backend must be verified before switching.");
	}
	if (origin === state.primaryOrigin) return { timedOut: false };
	updateState({ state: "switching", lastSwitchTimedOut: false });
	const timedOut = await waitForMutations();
	if (timedOut) {
		state = {
			...state,
			unknownMutationCount: state.unknownMutationCount + 1,
			lastSwitchTimedOut: true,
		};
	}
	const candidates = state.candidates.some((item) => item.origin === origin)
		? state.candidates.map((item) =>
				item.origin === origin
					? {
							...item,
							...candidate,
							verifiedAt: candidate.verifiedAt ?? Date.now(),
							lastError: null,
						}
					: item,
			)
		: [...state.candidates, candidate];
	state = {
		...state,
		primaryOrigin: origin,
		candidates,
		generation: state.generation + 1,
		state: "ready",
	};
	persistCurrentProfile();
	notify();
	return { timedOut };
}
