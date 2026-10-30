import {
	CapabilitiesResponseSchema,
	resolveApiCompatibility,
} from "@/api/apiCompatibility";
import type {
	BackendCandidate,
	PrimaryBackendSnapshot,
} from "./primaryBackend";

export function createBackendProber(context: {
	directFetch: typeof window.fetch;
	canonicalBackendOrigin: (value: string) => string | null;
	getState: () => PrimaryBackendSnapshot;
	getPageOrigin: () => string;
	allowedOrigins: () => string[];
	isPolicyExpired: () => boolean;
	isStaticWebConsole: () => boolean;
	getSessionEpoch: () => number;
}) {
	const { canonicalBackendOrigin } = context;
	async function directJson(
		origin: string,
		path: string,
		token?: string,
		signal?: AbortSignal,
	) {
		const response = await context.directFetch(new URL(path, origin), {
			method: "GET",
			redirect: "error",
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

	async function probeCompatibility(
		origin: string,
		signal?: AbortSignal,
	): Promise<void> {
		let compatibilityPayload: Record<string, unknown>;
		try {
			compatibilityPayload = CapabilitiesResponseSchema.parse(
				await directJson(origin, "/api/capabilities", undefined, signal),
			);
		} catch (error) {
			if ((error as { status?: number }).status !== 404) throw error;
			compatibilityPayload = (await directJson(
				origin,
				"/api/version/check",
				undefined,
				signal,
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
	}

	async function probeBackendOrigin(args: {
		origin: string;
		signal?: AbortSignal;
	}): Promise<{ clusterId: string }> {
		const origin = canonicalBackendOrigin(args.origin);
		if (!origin) throw new Error("Backend origin must be an HTTPS origin.");
		if (
			context.isStaticWebConsole() &&
			!context.allowedOrigins().includes(origin)
		) {
			throw new Error("Backend is not present in the active runtime policy.");
		}
		const clusterId = await probeBackendHealthAndCluster(origin, args.signal);
		await probeCompatibility(origin, args.signal);
		return { clusterId };
	}

	async function probeBackendHealthAndCluster(
		origin: string,
		signal?: AbortSignal,
	): Promise<string> {
		const health = await directJson(origin, "/api/health", undefined, signal);
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
			signal,
		);
		const clusterId =
			cluster && typeof cluster === "object"
				? (cluster as { cluster_id?: unknown }).cluster_id
				: undefined;
		if (typeof clusterId !== "string" || clusterId.length === 0) {
			throw new Error("Backend did not return a cluster identity.");
		}
		return clusterId;
	}

	async function probeBackendCandidate(args: {
		origin: string;
		clusterId: string;
		signal?: AbortSignal;
	}): Promise<void> {
		const origin = canonicalBackendOrigin(args.origin);
		if (!origin) throw new Error("Backend origin must be an HTTPS origin.");
		if (
			context.isStaticWebConsole() &&
			!context.allowedOrigins().includes(origin)
		) {
			throw new Error("Backend is not present in the active runtime policy.");
		}
		const clusterId = await probeBackendHealthAndCluster(origin, args.signal);
		if (clusterId !== args.clusterId) {
			throw new Error("Backend belongs to a different cluster.");
		}
		await probeCompatibility(origin, args.signal);
	}

	async function verifyBackendCandidate(args: {
		origin: string;
		clusterId: string;
		adminToken: string;
		signal?: AbortSignal;
	}): Promise<BackendCandidate> {
		const sessionEpoch = context.getSessionEpoch();
		const origin = canonicalBackendOrigin(args.origin);
		if (!origin) throw new Error("Backend origin must be an HTTPS origin.");
		if (context.isStaticWebConsole()) {
			if (context.isPolicyExpired())
				throw new Error(
					"Runtime policy has expired; backend switching is locked.",
				);
			const candidate = context
				.getState()
				.candidates.find((item) => item.origin === origin);
			if (candidate?.runtimeState === "temporarily_unavailable")
				throw new Error("Backend is temporarily unavailable.");
			if (candidate?.runtimeState === "staged")
				throw new Error(
					"Backend requires a confirmed reload before it can be checked.",
				);
		}
		await probeBackendCandidate(args);
		if (sessionEpoch !== context.getSessionEpoch()) {
			throw new Error("Backend session changed during verification.");
		}
		if (
			context.getState().clusterId !== args.clusterId ||
			(origin !== context.getPageOrigin() &&
				!context
					.getState()
					.candidates.some((candidate) => candidate.origin === origin))
		) {
			throw new Error("Backend candidate is no longer available.");
		}
		assertActiveStaticCandidate(origin);
		const nodes = await directJson(
			origin,
			"/api/admin/nodes",
			args.adminToken,
			args.signal,
		);
		if (
			sessionEpoch !== context.getSessionEpoch() ||
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
		if (!node && origin !== context.getPageOrigin()) {
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

	async function readSwitchCandidateResource(args: {
		origin: string;
		clusterId: string;
		adminToken: string;
		path: "/api/admin/nodes" | "/api/admin/console/runtime-policy";
	}): Promise<unknown> {
		const origin = canonicalBackendOrigin(args.origin);
		const generation = context.getState().generation;
		const sessionEpoch = context.getSessionEpoch();
		if (
			!origin ||
			context.getState().state !== "switching" ||
			context.getState().clusterId !== args.clusterId ||
			!context
				.getState()
				.candidates.some((candidate) => candidate.origin === origin)
		) {
			throw new Error("Backend switch candidate is no longer available.");
		}
		const signal = AbortSignal.timeout(8_000);
		await probeBackendCandidate({ origin, clusterId: args.clusterId, signal });
		if (
			context.getState().state !== "switching" ||
			context.getSessionEpoch() !== sessionEpoch ||
			context.getState().clusterId !== args.clusterId ||
			context.getState().generation !== generation ||
			!context
				.getState()
				.candidates.some((candidate) => candidate.origin === origin)
		) {
			throw new Error("Backend switch identity changed during verification.");
		}
		assertActiveStaticCandidate(origin);
		return directJson(origin, args.path, args.adminToken, signal);
	}
	function assertActiveStaticCandidate(origin: string): void {
		if (!context.isStaticWebConsole()) return;
		const candidate = context
			.getState()
			.candidates.find((item) => item.origin === origin);
		if (
			context.isPolicyExpired() ||
			!context.allowedOrigins().includes(origin) ||
			candidate?.runtimeState === "staged" ||
			candidate?.runtimeState === "temporarily_unavailable"
		) {
			throw new Error(
				"Backend is no longer available under the active runtime policy.",
			);
		}
	}
	return {
		probeBackendOrigin,
		probeBackendCandidate,
		verifyBackendCandidate,
		readSwitchCandidateResource,
	};
}
