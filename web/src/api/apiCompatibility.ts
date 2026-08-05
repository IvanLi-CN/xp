import { z } from "zod";

import { isBackendApiError, throwIfNotOk } from "./backendError";
import {
	API_CAPABILITIES_PATH,
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
	const knownFingerprintPaths = new Set(
		RELEASE_INVENTORIES.flatMap((candidate) =>
			Object.keys(candidate.fingerprint),
		),
	);
	for (const path of Object.keys(fingerprint)) {
		if (
			knownFingerprintPaths.has(path) &&
			inventory.fingerprint[path] === undefined
		) {
			return false;
		}
	}
	return Object.entries(inventory.fingerprint).every(([path, fields]) => {
		const observed = fingerprint[path];
		return (
			observed !== undefined &&
			fields.every((field) => observed.includes(field))
		);
	});
}

function fingerprintProbePaths() {
	return [
		...new Set(
			RELEASE_INVENTORIES.flatMap((profile) =>
				Object.keys(profile.fingerprint),
			),
		),
	].map((path) => ({
		path,
		auth: path.startsWith("/api/admin/"),
		optional: RELEASE_INVENTORIES.some(
			(profile) => profile.fingerprint[path] === undefined,
		),
	}));
}

function commonProfile(
	candidates: ReleaseInventory[],
): ReleaseInventory | null {
	if (candidates.length === 0) return null;
	const signatures = new Set(
		candidates.map((candidate) => candidate.capabilities.join("\u0000")),
	);
	return signatures.size === 1 ? (candidates[0] ?? null) : null;
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
		const profile = commonProfile(leastSpecificCandidates);
		if (profile) return compatibleResult(profile, advertised);
	}

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

	if (input.fingerprint) {
		const candidates = RELEASE_INVENTORIES.filter((profile) =>
			fingerprintMatches(input.fingerprint ?? {}, profile),
		);
		const mostSpecificSize = Math.max(
			...candidates.map(
				(candidate) => Object.keys(candidate.fingerprint).length,
			),
		);
		const mostSpecificCandidates = candidates.filter(
			(candidate) =>
				Object.keys(candidate.fingerprint).length === mostSpecificSize,
		);
		const profile = commonProfile(mostSpecificCandidates);
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
	const res = await fetch(API_CAPABILITIES_PATH, {
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
	const fingerprint: ApiFingerprint = {};
	for (const probe of fingerprintProbePaths()) {
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
		if (res.status === 404 && probe.optional) continue;
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
	} catch (error) {
		if (!isBackendApiError(error) || error.status !== 404) {
			return {
				kind: "incompatible",
				reason:
					"The capabilities endpoint returned an invalid or failed response",
			};
		}
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
