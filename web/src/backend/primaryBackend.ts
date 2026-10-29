import { resolveApiCompatibility } from "@/api/apiCompatibility";
import { isDemoRuntime } from "../demo/runtimeMode";
import {
	type BackendCandidate,
	type BackendProfile,
	type PrimaryBackendSnapshot,
	STATIC_BOOTSTRAP_ORIGIN,
	STATIC_WEB_ORIGIN,
	type StaticBackendRuntimeState,
	type StaticRuntimePolicyGrant,
	type SwitchOptions,
	type SwitchResult,
	canonicalBackendOrigin,
	displayNodeName,
	getPageOrigin,
	isAbortError,
	isApiUrl,
	normalizeReleaseVersion,
} from "./primaryBackendModel";
import {
	STORAGE_KEY,
	readStoredProfiles,
	writeStoredProfiles,
} from "./primaryBackendStorage";

export {
	type BackendCandidate,
	type BackendProfile,
	type PrimaryBackendSnapshot,
	type StaticBackendRuntimeState,
	type StaticRuntimePolicyGrant,
	type SwitchOptions,
	type SwitchResult,
	STATIC_BOOTSTRAP_ORIGIN,
	STATIC_WEB_ORIGIN,
	canonicalBackendOrigin,
	currentWebReleaseVersion,
	normalizeReleaseVersion,
	recognizedReleaseVersion,
} from "./primaryBackendModel";

export const PRIMARY_BACKEND_SWITCH_TIMEOUT_MS = 60_000;

type NativeFetch = typeof window.fetch;

const MUTATING_METHODS = new Set(["POST", "PUT", "PATCH", "DELETE"]);
const listeners = new Set<(snapshot: PrimaryBackendSnapshot) => void>();
const mutationWaiters = new Set<(timedOut: boolean) => void>();

let nativeFetch: NativeFetch | null = null;
let installed = false;

export function isStaticWebConsole(): boolean {
	return !isDemoRuntime() && getPageOrigin() === STATIC_WEB_ORIGIN;
}

function readInitialState(): PrimaryBackendSnapshot {
	const stored = readStoredProfiles(isStaticWebConsole());
	const active = stored.activeClusterId
		? stored.profiles?.[stored.activeClusterId]
		: undefined;
	return {
		clusterId: stored.activeClusterId ?? null,
		primaryOrigin: isStaticWebConsole()
			? STATIC_BOOTSTRAP_ORIGIN
			: getPageOrigin(),
		candidates:
			active?.candidates?.map((candidate) => ({
				...candidate,
				nodeName: displayNodeName(candidate.nodeName, candidate.origin),
			})) ?? [],
		generation: 0,
		state: "ready",
		pendingMutations: 0,
		unknownMutationCount: 0,
		lastSwitchTimedOut: false,
	};
}

let state: PrimaryBackendSnapshot = readInitialState();
let staticAllowedOrigins = new Set<string>();
let staticRuntimePolicyExpired = false;
let directoryLoaded = false;
let directorySessionEpoch = 0;
let directoryRequestSerial = 0;
let directoryAppliedSerial = 0;
let staticPolicyCandidates = new Map<string, BackendCandidate>();

function notify() {
	const snapshot = getPrimaryBackendSnapshot();
	for (const listener of listeners) listener(snapshot);
}

function persistCurrentProfile() {
	if (!state.clusterId) return;
	const stored = readStoredProfiles(isStaticWebConsole());
	const profile: BackendProfile = {
		clusterId: state.clusterId,
		primaryOrigin: state.primaryOrigin,
		candidates: state.candidates,
		updatedAt: Date.now(),
	};
	writeStoredProfiles(
		{
			activeClusterId: state.clusterId,
			profiles: { ...(stored.profiles ?? {}), [state.clusterId]: profile },
		},
		isStaticWebConsole(),
	);
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

export async function withPrimaryBackendMutation<T>(
	operation: () => Promise<T>,
): Promise<T> {
	const release = typeof window === "undefined" ? null : beginMutation();
	try {
		return await operation();
	} finally {
		release?.();
	}
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
	staticAllowedOrigins = isStaticWebConsole()
		? new Set<string>([STATIC_BOOTSTRAP_ORIGIN])
		: new Set<string>();
	staticRuntimePolicyExpired = false;
	directoryLoaded = false;
	staticPolicyCandidates.clear();
	directorySessionEpoch++;
	directoryRequestSerial = 0;
	directoryAppliedSerial = 0;
}

export function clearPrimaryBackendDirectory(
	options: { clearProfiles?: boolean } = {},
) {
	directorySessionEpoch++;
	directoryRequestSerial = 0;
	directoryAppliedSerial = 0;
	directoryLoaded = false;
	staticPolicyCandidates.clear();
	staticAllowedOrigins = new Set<string>();
	staticRuntimePolicyExpired = false;
	state = {
		...state,
		clusterId: null,
		primaryOrigin: isStaticWebConsole()
			? STATIC_BOOTSTRAP_ORIGIN
			: getPageOrigin(),
		candidates: [],
		generation: state.generation + 1,
		state: "ready",
	};
	if (options.clearProfiles && typeof window !== "undefined") {
		try {
			window.localStorage.removeItem(STORAGE_KEY);
		} catch {
			/* Storage may be unavailable. */
		}
	}
	notify();
}

export function getDirectorySessionEpoch(): number {
	return directorySessionEpoch;
}

export function hasDirectoryInventory(
	clusterId: string,
	epoch: number,
): boolean {
	return (
		directoryLoaded &&
		state.clusterId === clusterId &&
		epoch === directorySessionEpoch
	);
}

export function beginDirectoryRequest(): number {
	return ++directoryRequestSerial;
}

export function applyDirectoryResponse(
	clusterId: string,
	epoch: number,
	serial: number,
	nodes: Array<{ node_id: string; node_name: string; api_base_url: string }>,
): boolean {
	if (
		epoch !== directorySessionEpoch ||
		state.clusterId !== clusterId ||
		serial < directoryAppliedSerial
	)
		return false;
	directoryAppliedSerial = serial;
	hydratePrimaryBackendProfile(clusterId, nodes);
	return true;
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
	if (state.clusterId && state.clusterId !== clusterId) {
		clearPrimaryBackendDirectory();
	}
	if (nodes !== undefined) directoryLoaded = true;
	if (isStaticWebConsole()) {
		const candidatesByOrigin = new Map<string, BackendCandidate>();
		const bootstrapOrigin = canonicalBackendOrigin(STATIC_BOOTSTRAP_ORIGIN);
		const previousCandidates = new Map(
			state.candidates.map((candidate) => [candidate.origin, candidate]),
		);
		if (bootstrapOrigin && nodes === undefined && !directoryLoaded) {
			const existing = previousCandidates.get(bootstrapOrigin);
			candidatesByOrigin.set(bootstrapOrigin, {
				...existing,
				origin: bootstrapOrigin,
				nodeId: existing?.nodeId ?? "bootstrap",
				nodeName: existing?.nodeName ?? "Bootstrap backend",
				verifiedAt: existing?.verifiedAt ?? null,
				lastError: existing?.lastError ?? null,
				runtimeState: "active",
				switchable: true,
				switchBlockedReason: undefined,
			});
		}
		if (nodes === undefined) {
			for (const candidate of state.candidates) {
				candidatesByOrigin.set(candidate.nodeId, candidate);
			}
		}
		for (const node of nodes ?? []) {
			const rawOrigin = node.api_base_url.trim();
			const origin = canonicalBackendOrigin(rawOrigin);
			const candidateKey = node.node_id;
			const existing = state.candidates.find(
				(candidate) =>
					candidate.nodeId === node.node_id &&
					candidate.origin === (origin ?? rawOrigin),
			);
			const policy = origin ? staticPolicyCandidates.get(origin) : undefined;
			const runtimeState = policy?.runtimeState;
			const switchable =
				origin !== null &&
				staticAllowedOrigins.has(origin) &&
				runtimeState !== "staged" &&
				runtimeState !== "temporarily_unavailable";
			candidatesByOrigin.set(candidateKey, {
				...existing,
				origin: origin ?? rawOrigin,
				nodeId: node.node_id,
				nodeName: displayNodeName(node.node_name, origin ?? rawOrigin),
				verifiedAt: existing?.verifiedAt ?? null,
				lastError: existing?.lastError ?? null,
				switchable,
				switchBlockedReason:
					origin === null
						? "HTTPS is required for this node."
						: runtimeState === "staged"
							? "This node requires a confirmed reload."
							: runtimeState === "temporarily_unavailable"
								? "This node is temporarily unavailable."
								: switchable
									? undefined
									: "This node is not available to the static console.",
				runtimeState,
				runtimePolicyId: policy?.runtimePolicyId,
				runtimePolicyGrant: policy?.runtimePolicyGrant,
			});
		}
		const currentOrigin = canonicalBackendOrigin(state.primaryOrigin);
		const nextPrimary =
			(currentOrigin &&
			staticAllowedOrigins.has(currentOrigin) &&
			[...candidatesByOrigin.values()].some(
				(candidate) => candidate.origin === currentOrigin,
			)
				? currentOrigin
				: bootstrapOrigin) ?? getPageOrigin();
		const changed = state.clusterId !== clusterId;
		state = {
			...state,
			clusterId,
			primaryOrigin: nextPrimary,
			candidates: [...candidatesByOrigin.values()],
			generation:
				changed || state.primaryOrigin !== nextPrimary
					? state.generation + 1
					: state.generation,
		};
		notify();
		return;
	}
	const stored = readStoredProfiles(isStaticWebConsole());
	const storedProfile = stored.profiles?.[clusterId];
	const candidatesByOrigin = new Map<string, BackendCandidate>();
	if (nodes === undefined) {
		const existingRows = directoryLoaded
			? state.candidates
			: (storedProfile?.candidates ?? []);
		for (const candidate of existingRows) {
			if (directoryLoaded || canonicalBackendOrigin(candidate.origin)) {
				candidatesByOrigin.set(candidate.nodeId, {
					...candidate,
					nodeName: displayNodeName(candidate.nodeName, candidate.origin),
				});
			}
		}
	}
	for (const node of nodes ?? []) {
		const rawOrigin = node.api_base_url.trim();
		const origin = canonicalBackendOrigin(rawOrigin);
		const candidateKey = node.node_id;
		const existing = state.candidates.find(
			(candidate) =>
				candidate.nodeId === node.node_id &&
				candidate.origin === (origin ?? rawOrigin),
		);
		candidatesByOrigin.set(candidateKey, {
			...existing,
			origin: origin ?? rawOrigin,
			nodeId: node.node_id,
			nodeName: displayNodeName(node.node_name, origin ?? rawOrigin),
			verifiedAt: existing?.verifiedAt ?? null,
			lastError: existing?.lastError ?? null,
			switchable: origin !== null,
			switchBlockedReason:
				origin === null ? "HTTPS is required for this node." : undefined,
		});
	}
	const pageOrigin = getPageOrigin();
	if (pageOrigin && nodes === undefined && !directoryLoaded) {
		if (
			![...candidatesByOrigin.values()].some(
				(candidate) => candidate.origin === pageOrigin,
			)
		) {
			candidatesByOrigin.set(pageOrigin, {
				origin: pageOrigin,
				nodeId: "current-page",
				nodeName: "Current page",
				verifiedAt: Date.now(),
				lastError: null,
				switchable: true,
			});
		}
	}
	const persistedPrimary = canonicalBackendOrigin(
		storedProfile?.primaryOrigin ?? "",
	);
	const nextPrimary =
		persistedPrimary &&
		(nodes === undefined ||
			[...candidatesByOrigin.values()].some(
				(candidate) => candidate.origin === persistedPrimary,
			))
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
		state: state.state === "switching" ? "switching" : "ready",
	};
	persistCurrentProfile();
	notify();
}

export function hydrateStaticRuntimePolicy(
	clusterId: string,
	origins: readonly string[],
	options: {
		nodeStates?: readonly {
			origin: string;
			state: "allowed" | "temporarily_unavailable";
		}[];
		candidateOrigins?: readonly string[];
		policyId?: string;
		policyGrant?: StaticRuntimePolicyGrant;
		activate?: boolean;
	} = {},
) {
	if (state.clusterId && state.clusterId !== clusterId) {
		clearPrimaryBackendDirectory();
	}
	const bootstrapOrigin = canonicalBackendOrigin(STATIC_BOOTSTRAP_ORIGIN);
	if (options.activate) {
		staticRuntimePolicyExpired = false;
		staticAllowedOrigins = bootstrapOrigin
			? new Set<string>([bootstrapOrigin])
			: new Set<string>();
		for (const rawOrigin of origins) {
			const origin = canonicalBackendOrigin(rawOrigin);
			if (origin) staticAllowedOrigins.add(origin);
		}
	} else if (bootstrapOrigin) {
		staticAllowedOrigins.add(bootstrapOrigin);
	}
	const candidatesByOrigin = new Map<string, BackendCandidate>();
	const previousCandidates = new Map(
		state.candidates.map((candidate) => [candidate.origin, candidate]),
	);
	if (bootstrapOrigin) {
		const existing = previousCandidates.get(bootstrapOrigin);
		candidatesByOrigin.set(bootstrapOrigin, {
			...existing,
			origin: bootstrapOrigin,
			nodeId: existing?.nodeId ?? "bootstrap",
			nodeName: existing?.nodeName ?? "Bootstrap backend",
			verifiedAt: existing?.verifiedAt ?? null,
			lastError: existing?.lastError ?? null,
			switchable: true,
			switchBlockedReason: undefined,
			runtimeState: "active",
		});
	}
	const nodeStates = new Map<string, "allowed" | "temporarily_unavailable">();
	for (const node of options.nodeStates ?? []) {
		const origin = canonicalBackendOrigin(node.origin);
		if (origin) nodeStates.set(origin, node.state);
	}
	for (const rawOrigin of options.candidateOrigins ?? origins) {
		const origin = canonicalBackendOrigin(rawOrigin);
		if (!origin) continue;
		if (!nodeStates.has(origin)) nodeStates.set(origin, "allowed");
		const existing = previousCandidates.get(origin);
		const policyManaged = Boolean(
			options.policyId ||
				options.policyGrant ||
				options.activate ||
				options.nodeStates?.length,
		);
		const runtimeState: StaticBackendRuntimeState = !policyManaged
			? "active"
			: nodeStates.get(origin) === "temporarily_unavailable"
				? "temporarily_unavailable"
				: staticAllowedOrigins.has(origin)
					? "active"
					: "staged";
		const switchable = !policyManaged || runtimeState === "active";
		candidatesByOrigin.set(origin, {
			...existing,
			origin,
			nodeId: existing?.nodeId ?? origin,
			nodeName: existing?.nodeName ?? "Verified backend",
			verifiedAt: existing?.verifiedAt ?? Date.now(),
			lastError:
				runtimeState === "temporarily_unavailable"
					? "Backend is temporarily unavailable."
					: null,
			runtimeState,
			runtimePolicyId: options.policyId,
			runtimePolicyGrant: options.policyGrant,
			switchable,
			switchBlockedReason: switchable
				? undefined
				: "This node is not available to the static console.",
		});
	}
	for (const origin of [...staticAllowedOrigins]) {
		if (origin !== bootstrapOrigin && nodeStates.get(origin) !== "allowed") {
			staticAllowedOrigins.delete(origin);
		}
	}
	staticPolicyCandidates = candidatesByOrigin;
	const candidates =
		directoryLoaded && state.clusterId === clusterId
			? state.candidates.map((candidate) => {
					const origin = canonicalBackendOrigin(candidate.origin);
					const policy = origin ? candidatesByOrigin.get(origin) : undefined;
					return {
						...candidate,
						runtimeState: policy?.runtimeState,
						runtimePolicyId: policy?.runtimePolicyId,
						runtimePolicyGrant: policy?.runtimePolicyGrant,
						switchable: Boolean(policy?.switchable),
						switchBlockedReason: policy?.switchable
							? undefined
							: (policy?.switchBlockedReason ??
								(origin
									? "This node is not available to the static console."
									: "HTTPS is required for this node.")),
					};
				})
			: [...candidatesByOrigin.values()];
	const currentOrigin = canonicalBackendOrigin(state.primaryOrigin);
	const currentCandidate = currentOrigin
		? candidates.find((candidate) => candidate.origin === currentOrigin)
		: undefined;
	const nextPrimary =
		(currentOrigin &&
		currentCandidate?.runtimeState === "active" &&
		staticAllowedOrigins.has(currentOrigin)
			? currentOrigin
			: bootstrapOrigin) ?? getPageOrigin();
	const changed =
		state.clusterId !== clusterId || state.primaryOrigin !== nextPrimary;
	state = {
		...state,
		clusterId,
		primaryOrigin: nextPrimary,
		candidates,
		generation: changed ? state.generation + 1 : state.generation,
		state: state.state === "switching" ? "switching" : "ready",
	};
	staticRuntimePolicyExpired = false;
	notify();
}

export function expireStaticRuntimePolicy(clusterId?: string) {
	if (!isStaticWebConsole()) return;
	staticRuntimePolicyExpired = true;
	if (clusterId && state.clusterId !== clusterId) {
		state = { ...state, clusterId };
	}
	notify();
}

export function isStaticRuntimePolicyExpired(): boolean {
	return staticRuntimePolicyExpired;
}

export function getStaticRuntimePolicyOrigins(): string[] {
	return [...staticAllowedOrigins].sort();
}

export function hydrateBackendVersions(
	items: ReadonlyArray<{
		node_id: string;
		xp_version: string | null;
		error: string | null;
	}>,
) {
	const versions = new Map(items.map((item) => [item.node_id, item]));
	state = {
		...state,
		candidates: state.candidates.map((candidate) => {
			const item = versions.get(candidate.nodeId);
			if (!item) {
				return {
					...candidate,
					serviceVersion: null,
					versionState: "unknown",
					lastError: "unknown",
				};
			}
			return {
				...candidate,
				serviceVersion: item.xp_version
					? normalizeReleaseVersion(item.xp_version)
					: null,
				versionState: item.xp_version ? "ready" : "unavailable",
				lastError: item.error,
			};
		}),
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
	const sessionEpoch = directorySessionEpoch;
	const origin = canonicalBackendOrigin(args.origin);
	if (!origin) throw new Error("Backend origin must be an HTTPS origin.");
	if (isStaticWebConsole()) {
		if (staticRuntimePolicyExpired) {
			throw new Error(
				"Runtime policy has expired; backend switching is locked.",
			);
		}
		const candidate = state.candidates.find((item) => item.origin === origin);
		if (candidate?.runtimeState === "temporarily_unavailable") {
			throw new Error("Backend is temporarily unavailable.");
		}
		if (candidate?.runtimeState === "staged") {
			throw new Error(
				"Backend requires a confirmed reload before it can be checked.",
			);
		}
		if (!staticAllowedOrigins.has(origin)) {
			throw new Error("Backend is not present in the active runtime policy.");
		}
	}
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
	if (sessionEpoch !== directorySessionEpoch) {
		throw new Error("Backend session changed during verification.");
	}
	const nodes = await directJson(
		origin,
		"/api/admin/nodes",
		args.adminToken,
		args.signal,
	);
	if (
		sessionEpoch !== directorySessionEpoch ||
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
	const deadline = Date.now() + PRIMARY_BACKEND_SWITCH_TIMEOUT_MS;
	while (state.pendingMutations > 0) {
		const remainingMs = deadline - Date.now();
		if (remainingMs <= 0) return true;
		const timedOut = await new Promise<boolean>((resolve) => {
			let settled = false;
			const settle = (didTimeOut: boolean) => {
				if (settled) return;
				settled = true;
				window.clearTimeout(timeoutId);
				mutationWaiters.delete(settle);
				resolve(didTimeOut);
			};
			const timeoutId = window.setTimeout(() => settle(true), remainingMs);
			mutationWaiters.add(settle);
		});
		if (timedOut) return true;
	}
	return false;
}

export async function switchPrimaryBackend(
	candidate: BackendCandidate,
	options: SwitchOptions = {},
): Promise<SwitchResult> {
	const sessionEpoch = directorySessionEpoch;
	const origin = canonicalBackendOrigin(candidate.origin);
	if (!origin) throw new Error("Backend origin must be an HTTPS origin.");
	if (!candidate.verifiedAt) {
		throw new Error("Backend must be verified before switching.");
	}
	if (isStaticWebConsole()) {
		if (staticRuntimePolicyExpired) {
			throw new Error(
				"Runtime policy has expired; backend switching is locked.",
			);
		}
		if (candidate.runtimeState === "staged") {
			throw new Error("Backend requires a confirmed reload before switching.");
		}
		if (candidate.runtimeState === "temporarily_unavailable") {
			throw new Error("Backend is temporarily unavailable.");
		}
	}
	if (state.state === "switching") {
		throw new Error("Backend switch is already in progress.");
	}
	if (origin === state.primaryOrigin) return { timedOut: false };
	updateState({ state: "switching", lastSwitchTimedOut: false });
	const barrierTimedOut = await waitForMutations();
	if (sessionEpoch !== directorySessionEpoch) {
		throw new Error("Backend session changed during switching.");
	}
	if (barrierTimedOut) {
		state = {
			...state,
			unknownMutationCount: state.unknownMutationCount + 1,
			lastSwitchTimedOut: true,
		};
	}
	let revalidated = true;
	if (options.revalidate) {
		try {
			revalidated = await options.revalidate(candidate);
		} catch {
			revalidated = false;
		}
	}
	if (sessionEpoch !== directorySessionEpoch) {
		throw new Error("Backend session changed during switching.");
	}
	if (!revalidated) {
		state = {
			...state,
			state: "ready",
		};
		notify();
		throw new Error("Backend directory is no longer available for switching.");
	}
	if (options.revalidate && !barrierTimedOut && state.pendingMutations > 0) {
		const revalidationBarrierTimedOut = await waitForMutations();
		if (sessionEpoch !== directorySessionEpoch) {
			throw new Error("Backend session changed during switching.");
		}
		if (revalidationBarrierTimedOut || state.pendingMutations > 0) {
			state = {
				...state,
				state: "ready",
				unknownMutationCount:
					state.unknownMutationCount + (revalidationBarrierTimedOut ? 1 : 0),
				lastSwitchTimedOut: revalidationBarrierTimedOut,
			};
			notify();
			throw new Error("Backend switch is blocked by an unresolved mutation.");
		}
	}
	const currentCandidate = state.candidates.find(
		(item) => item.origin === origin && item.nodeId === candidate.nodeId,
	);
	const policyManaged =
		isStaticWebConsole() || currentCandidate?.runtimePolicyId !== undefined;
	if (
		sessionEpoch !== directorySessionEpoch ||
		!currentCandidate ||
		currentCandidate.switchable === false ||
		(policyManaged &&
			(currentCandidate.runtimeState === "temporarily_unavailable" ||
				currentCandidate.runtimeState === "staged"))
	) {
		state = {
			...state,
			state: "ready",
		};
		notify();
		throw new Error("Backend is no longer available for switching.");
	}
	const candidates = state.candidates.some((item) => item.origin === origin)
		? state.candidates.map((item) =>
				item.origin === origin
					? {
							...item,
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
	return { timedOut: barrierTimedOut };
}
