import type { AdminEndpoint } from "../api/adminEndpoints";
import {
	normalizeAcceptedAuthority,
	validateAcceptedAuthority,
} from "./acceptedAuthority";

export type ManagedVlessAutocompleteSuggestion = {
	value: string;
	label: string;
	description?: string;
};

export type ManagedVlessSuggestionScope = {
	nodeId: string | null | undefined;
	accessHost?: string | null | undefined;
	apiBaseUrl?: string | null | undefined;
};

export const MANAGED_VLESS_MODE_HELPER_TEXT =
	"Use an origin URL only. WebSocket uses HTTP/1.1 upstream; h2c is for " +
	"non-upgrade HTTP.";

export const MANAGED_VLESS_ACCEPTED_HOST_HELPER_TEXT =
	"Accept additional ordinary HTTPS Host headers for camouflage routing. " +
	"Omit port to use HTTPS default 443. This does not change REALITY " +
	"serverNames or the canonical /generate_204 URL.";

export function dedupeAcceptedAuthorities(values: string[]): string[] {
	return [...new Set(values)];
}

export function normalizeAcceptedAuthorities(values: string[]): string[] {
	return dedupeAcceptedAuthorities(
		values
			.map(normalizeAcceptedAuthority)
			.filter((authority) => authority.length > 0),
	);
}

export function findAcceptedAuthorityError(values: string[]): string | null {
	return (
		normalizeAcceptedAuthorities(values)
			.map((authority) => validateAcceptedAuthority(authority))
			.find((error): error is string => Boolean(error)) ?? null
	);
}

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null;
}

function normalizeCanaryUpstreamOrigin(
	value: string | null | undefined,
): string | null {
	const trimmed = value?.trim() ?? "";
	if (!trimmed) return null;

	let url: URL;
	try {
		url = new URL(trimmed);
	} catch {
		return null;
	}

	if (
		(url.protocol !== "http:" && url.protocol !== "https:") ||
		!url.hostname
	) {
		return null;
	}

	return url.origin;
}

function normalizeHostname(value: string): string {
	return value
		.trim()
		.replace(/^\[(.+)\]$/, "$1")
		.toLowerCase();
}

function hostnameFromOrigin(value: string | null | undefined): string | null {
	const origin = normalizeCanaryUpstreamOrigin(value);
	if (!origin) return null;

	try {
		return normalizeHostname(new URL(origin).hostname);
	} catch {
		return null;
	}
}

function hostnameFromAcceptedAuthority(
	value: string | null | undefined,
): string | null {
	const normalized = normalizeAcceptedAuthority(value ?? "");
	if (!normalized || validateAcceptedAuthority(normalized)) return null;

	if (normalized.startsWith("[")) {
		const end = normalized.indexOf("]");
		return end === -1 ? null : normalizeHostname(normalized.slice(1, end));
	}

	const splitIndex = normalized.lastIndexOf(":");
	return normalizeHostname(
		splitIndex === -1 ? normalized : normalized.slice(0, splitIndex),
	);
}

function nodeOwnedCanaryUpstreamHosts(
	scope:
		| Pick<ManagedVlessSuggestionScope, "accessHost" | "apiBaseUrl">
		| null
		| undefined,
): Set<string> {
	const blockedHosts = new Set<string>();
	const accessHost = hostnameFromAcceptedAuthority(scope?.accessHost);
	if (accessHost) blockedHosts.add(accessHost);
	const apiBaseUrlHost = hostnameFromOrigin(scope?.apiBaseUrl);
	if (apiBaseUrlHost) blockedHosts.add(apiBaseUrlHost);
	return blockedHosts;
}

export function canaryUpstreamSuggestionsFromUrls(
	values: Iterable<string | null | undefined>,
	scope?: Pick<ManagedVlessSuggestionScope, "accessHost" | "apiBaseUrl"> | null,
): ManagedVlessAutocompleteSuggestion[] {
	const suggestions: ManagedVlessAutocompleteSuggestion[] = [];
	const seen = new Set<string>();
	const blockedHosts = nodeOwnedCanaryUpstreamHosts(scope);

	for (const value of values) {
		const normalized = normalizeCanaryUpstreamOrigin(value);
		if (!normalized || seen.has(normalized)) continue;
		const hostname = hostnameFromOrigin(normalized);
		if (hostname && blockedHosts.has(hostname)) continue;
		seen.add(normalized);
		suggestions.push({ value: normalized, label: normalized });
	}

	return suggestions;
}

export function canaryUpstreamSuggestionsFromManagedEndpoints(
	endpoints: readonly Pick<
		AdminEndpoint,
		"endpoint_id" | "node_id" | "kind" | "meta"
	>[],
	scope: ManagedVlessSuggestionScope,
): ManagedVlessAutocompleteSuggestion[] {
	const trimmedNodeId = scope.nodeId?.trim() ?? "";
	if (!trimmedNodeId) return [];

	return canaryUpstreamSuggestionsFromUrls(
		endpoints.flatMap((endpoint) => {
			if (
				endpoint.kind !== "vless_reality_vision_tcp" ||
				endpoint.node_id !== trimmedNodeId
			) {
				return [];
			}

			const meta = isRecord(endpoint.meta) ? endpoint.meta : null;
			if (!meta || meta.managed_default !== true) return [];

			const upstream = isRecord(meta.canary_upstream)
				? meta.canary_upstream
				: null;
			return typeof upstream?.url === "string" ? [upstream.url] : [];
		}),
		scope,
	);
}

export function acceptedAuthoritySuggestionsFromAccessHost(
	accessHost: string | null | undefined,
): string[] {
	const trimmed = accessHost?.trim() ?? "";
	if (!trimmed) return [];

	const normalized = normalizeAcceptedAuthority(trimmed);
	if (!normalized || validateAcceptedAuthority(normalized)) return [];

	return normalized.endsWith(":443")
		? [normalized.slice(0, -":443".length)]
		: [normalized];
}
