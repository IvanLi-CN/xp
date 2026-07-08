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

export function canaryUpstreamSuggestionsFromUrls(
	values: Iterable<string | null | undefined>,
): ManagedVlessAutocompleteSuggestion[] {
	const suggestions: ManagedVlessAutocompleteSuggestion[] = [];
	const seen = new Set<string>();

	for (const value of values) {
		const normalized = normalizeCanaryUpstreamOrigin(value);
		if (!normalized || seen.has(normalized)) continue;
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
	nodeId: string | null | undefined,
): ManagedVlessAutocompleteSuggestion[] {
	const trimmedNodeId = nodeId?.trim() ?? "";
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
