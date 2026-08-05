import { useQuery } from "@tanstack/react-query";
import {
	type ReactNode,
	createContext,
	createElement,
	useContext,
} from "react";

import {
	type ApiCompatibilityResult,
	fetchApiCompatibility,
	resolveApiCompatibility,
} from "./apiCompatibility";
import { type ApiCapability, RELEASE_INVENTORIES } from "./releaseInventories";

const PERSISTED_COMPATIBILITY_PREFIX = "xp-api-compatibility-";

type PersistedCompatibility =
	| {
			kind: "compatible";
			profileReleaseTag: string;
			advertisedCapabilities: string[];
	  }
	| { kind: "incompatible"; reason: string };

function persistedCompatibilityKey(adminToken: string): string {
	return `${PERSISTED_COMPATIBILITY_PREFIX}${adminToken ? "admin" : "anonymous"}`;
}

function readPersistedCompatibility(
	adminToken: string,
): ApiCompatibilityResult | undefined {
	if (typeof localStorage === "undefined") return undefined;
	try {
		const raw = localStorage.getItem(persistedCompatibilityKey(adminToken));
		if (!raw) return undefined;
		const snapshot = JSON.parse(raw) as PersistedCompatibility;
		if (snapshot.kind === "incompatible") return snapshot;
		const profile = RELEASE_INVENTORIES.find(
			(candidate) => candidate.releaseTag === snapshot.profileReleaseTag,
		);
		if (
			!profile ||
			!Array.isArray(snapshot.advertisedCapabilities) ||
			snapshot.advertisedCapabilities.some(
				(capability) => typeof capability !== "string",
			)
		) {
			return undefined;
		}
		return resolveApiCompatibility({
			releaseTag: profile.releaseTag,
			capabilities: snapshot.advertisedCapabilities,
		});
	} catch {
		return undefined;
	}
}

function persistCompatibility(
	adminToken: string,
	compatibility: ApiCompatibilityResult,
): void {
	if (typeof localStorage === "undefined") return;
	const snapshot: PersistedCompatibility =
		compatibility.kind === "incompatible"
			? compatibility
			: {
					kind: "compatible",
					profileReleaseTag: compatibility.profile.releaseTag,
					advertisedCapabilities: [...compatibility.advertisedCapabilities],
				};
	try {
		localStorage.setItem(
			persistedCompatibilityKey(adminToken),
			JSON.stringify(snapshot),
		);
	} catch {
		// Capability persistence is an offline enhancement, not a hard dependency.
	}
}

// Undefined means an embedded component was rendered without the app provider.
// AppShell explicitly supplies null while the compatibility probe is pending.
const ApiCompatibilityContext = createContext<
	ApiCompatibilityResult | null | undefined
>(undefined);

export function ApiCompatibilityProvider(props: {
	value: ApiCompatibilityResult | null;
	children: ReactNode;
}) {
	return createElement(
		ApiCompatibilityContext.Provider,
		{ value: props.value },
		props.children,
	);
}

export function useApiCompatibility(adminToken: string, isOnline: boolean) {
	return useQuery({
		queryKey: ["apiCompatibility", adminToken ? "admin" : "anonymous"],
		queryFn: async ({ signal }) => {
			const compatibility = await fetchApiCompatibility({
				adminToken: adminToken || undefined,
				signal,
			});
			persistCompatibility(adminToken, compatibility);
			return compatibility;
		},
		enabled: isOnline,
		staleTime: 5 * 60 * 1000,
		retry: false,
		// Online sessions must wait for a probe against the current server. The
		// snapshot is only an offline fallback and is intentionally not keyed to a
		// PWA build or cache identity.
		initialData: isOnline
			? undefined
			: () => readPersistedCompatibility(adminToken),
		initialDataUpdatedAt: 0,
	});
}

export function getApiCapabilityState(
	compatibility: ApiCompatibilityResult | null | undefined,
	capability: string,
) {
	const pending = compatibility === null;
	const unavailable =
		pending ||
		compatibility?.kind === "incompatible" ||
		(compatibility?.kind === "compatible" &&
			!compatibility.isFeatureAvailable(capability));
	return {
		available: !unavailable,
		unavailable,
		pending,
		reason: pending
			? "Checking API compatibility with the connected server."
			: compatibility?.kind === "incompatible"
				? compatibility.reason
				: compatibility?.kind === "compatible"
					? compatibility.degradationFor(capability)
					: null,
	};
}

export function useApiCapability(capability: ApiCapability) {
	const compatibility = useContext(ApiCompatibilityContext);
	return getApiCapabilityState(compatibility, capability);
}
