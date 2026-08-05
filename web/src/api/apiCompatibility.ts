import { z } from "zod";

import { throwIfNotOk } from "./backendError";
import {
	API_CAPABILITIES,
	API_COMPATIBILITY_WINDOW,
	type ApiCapability,
	type ApiCompatibilityMinor,
	RELEASE_INVENTORIES,
	type ReleaseInventory,
} from "./releaseInventories";
import { fetchVersionCheck } from "./versionCheck";

export { API_COMPATIBILITY_WINDOW } from "./releaseInventories";

const ReleaseTagSchema = z.string().regex(/^v\d+\.\d+\.\d+$/);
const CapabilitiesResponseSchema = z
	.object({
		release_tag: z.string().optional(),
		capabilities: z.array(z.string()).optional(),
		fingerprint: z.record(z.string(), z.array(z.string())).optional(),
	})
	.passthrough();

export type ApiFingerprint = Record<string, string[]>;

export type ApiCompatibilityResult =
	| {
			kind: "compatible";
			profile: ReleaseInventory;
			advertisedCapabilities: ReadonlySet<string>;
			isFeatureAvailable: (capability: string) => boolean;
			degradationFor: (capability: string) => string | null;
	  }
	| {
			kind: "incompatible";
			reason: string;
	  };

export function resolveReleaseMinor(
	releaseTag: string,
): ApiCompatibilityMinor | null {
	if (!ReleaseTagSchema.safeParse(releaseTag).success) return null;
	const match = /^v(\d+)\.(\d+)\./.exec(releaseTag);
	const minor = match ? `${match[1]}.${match[2]}` : "";
	return API_COMPATIBILITY_WINDOW.includes(minor as ApiCompatibilityMinor)
		? (minor as ApiCompatibilityMinor)
		: null;
}

function fingerprintMatches(
	fingerprint: ApiFingerprint,
	inventory: ReleaseInventory,
): boolean {
	return Object.entries(inventory.fingerprint).every(([path, fields]) => {
		const observed = fingerprint[path];
		return (
			observed !== undefined &&
			fields.every((field) => observed.includes(field))
		);
	});
}

function commonProfile(
	candidates: ReleaseInventory[],
): ReleaseInventory | null {
	if (candidates.length === 0) return null;
	const first = candidates[0];
	if (!first) return null;
	const signature = first.capabilities.join("\u0000");
	return candidates.every(
		(candidate) => candidate.capabilities.join("\u0000") === signature,
	)
		? first
		: null;
}

function compatibleResult(
	profile: ReleaseInventory,
	advertisedCapabilities: Iterable<string>,
): ApiCompatibilityResult {
	const capabilities = new Set(advertisedCapabilities);
	return {
		kind: "compatible",
		profile,
		advertisedCapabilities: capabilities,
		isFeatureAvailable: (capability) => capabilities.has(capability),
		degradationFor: (capability) =>
			capabilities.has(capability)
				? null
				: `API capability is unavailable for this server: ${capability}`,
	};
}

export function resolveApiCompatibility(input: {
	releaseTag?: string;
	capabilities?: readonly string[];
	fingerprint?: ApiFingerprint;
}): ApiCompatibilityResult {
	const advertised = new Set(input.capabilities ?? []);
	if (input.releaseTag !== undefined) {
		const minor = resolveReleaseMinor(input.releaseTag);
		if (!minor) {
			return {
				kind: "incompatible",
				reason: `Unsupported or invalid release tag: ${input.releaseTag}`,
			};
		}
		const profile = RELEASE_INVENTORIES.find(
			(candidate) => candidate.minor === minor,
		);
		if (!profile) {
			return {
				kind: "incompatible",
				reason: `No inventory for minor ${minor}`,
			};
		}
		return compatibleResult(
			profile,
			advertised.size > 0 ? advertised : profile.capabilities,
		);
	}

	if (advertised.size > 0) {
		const candidates = RELEASE_INVENTORIES.filter((profile) =>
			[...advertised].every((capability) =>
				profile.capabilities.includes(capability as ApiCapability),
			),
		);
		const smallestProfileSize = Math.min(
			...candidates.map((candidate) => candidate.capabilities.length),
		);
		const leastSpecificCandidates = candidates.filter(
			(candidate) => candidate.capabilities.length === smallestProfileSize,
		);
		const profile =
			leastSpecificCandidates.length === 1
				? leastSpecificCandidates[0]
				: commonProfile(leastSpecificCandidates);
		if (profile) return compatibleResult(profile, advertised);
		return {
			kind: "incompatible",
			reason: "Capabilities do not identify an equivalent release profile",
		};
	}

	if (input.fingerprint) {
		const candidates = RELEASE_INVENTORIES.filter((profile) =>
			fingerprintMatches(input.fingerprint ?? {}, profile),
		);
		const profile =
			candidates.length === 1 ? candidates[0] : commonProfile(candidates);
		if (profile) return compatibleResult(profile, profile.capabilities);
	}

	return {
		kind: "incompatible",
		reason: "The server did not match an immutable API release inventory",
	};
}

async function fetchCapabilities(options: {
	adminToken?: string;
	signal?: AbortSignal;
}) {
	const res = await fetch("/api/capabilities", {
		method: "GET",
		headers: {
			Accept: "application/json",
			...(options.adminToken
				? { Authorization: `Bearer ${options.adminToken}` }
				: {}),
		},
		signal: options.signal,
	});
	await throwIfNotOk(res);
	return CapabilitiesResponseSchema.parse(await res.json());
}

async function fetchApiFingerprint(options: {
	adminToken?: string;
	signal?: AbortSignal;
}): Promise<ApiFingerprint> {
	const probes = [
		{ path: "/api/health", auth: false },
		{ path: "/api/cluster/info", auth: false },
		{ path: "/api/admin/nodes", auth: true },
	] as const;
	const fingerprint: ApiFingerprint = {};
	for (const probe of probes) {
		const res = await fetch(probe.path, {
			method: "GET",
			headers: {
				Accept: "application/json",
				...(probe.auth && options.adminToken
					? { Authorization: `Bearer ${options.adminToken}` }
					: {}),
			},
			signal: options.signal,
		});
		await throwIfNotOk(res);
		const payload = (await res.json()) as unknown;
		if (
			typeof payload !== "object" ||
			payload === null ||
			Array.isArray(payload)
		) {
			throw new Error(`Invalid fingerprint payload from ${probe.path}`);
		}
		fingerprint[probe.path] = Object.keys(
			payload as Record<string, unknown>,
		).sort();
	}
	return fingerprint;
}

export async function fetchApiCompatibility(options?: {
	adminToken?: string;
	signal?: AbortSignal;
}): Promise<ApiCompatibilityResult> {
	const request = { adminToken: options?.adminToken, signal: options?.signal };
	try {
		const capabilities = await fetchCapabilities(request);
		return resolveApiCompatibility({
			releaseTag: capabilities.release_tag,
			capabilities: capabilities.capabilities,
			fingerprint: capabilities.fingerprint,
		});
	} catch {
		try {
			const version = await fetchVersionCheck({ signal: options?.signal });
			return resolveApiCompatibility({
				releaseTag: version.current.release_tag,
			});
		} catch {
			try {
				return resolveApiCompatibility({
					fingerprint: await fetchApiFingerprint(request),
				});
			} catch {
				return {
					kind: "incompatible",
					reason: "Unable to identify the server API release profile",
				};
			}
		}
	}
}

export function capabilityFromRoute(route: string): ApiCapability | null {
	if (route === "GET /api/admin/status/events") return "admin.status-events";
	if (route === "GET /api/health") return "api.health";
	if (route === "GET /api/cluster/info") return "api.cluster-info";
	return (
		API_CAPABILITIES.find((capability) => route.includes(capability)) ?? null
	);
}
