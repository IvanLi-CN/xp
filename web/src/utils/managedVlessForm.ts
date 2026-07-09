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

function normalizeHttpsOriginFromAuthority(
	value: string | null | undefined,
): string | null {
	const trimmed = value?.trim() ?? "";
	if (!trimmed) return null;

	const normalized = normalizeAcceptedAuthority(trimmed);
	if (!normalized || validateAcceptedAuthority(normalized)) {
		return null;
	}

	return normalized.endsWith(":443")
		? `https://${normalized.slice(0, -":443".length)}`
		: `https://${normalized}`;
}

export function canaryUpstreamSuggestionsFromAuthorities(
	values: Iterable<string | null | undefined>,
): ManagedVlessAutocompleteSuggestion[] {
	const suggestions: ManagedVlessAutocompleteSuggestion[] = [];
	const seen = new Set<string>();

	for (const value of values) {
		const origin = normalizeHttpsOriginFromAuthority(value);
		if (!origin || seen.has(origin)) continue;
		seen.add(origin);
		suggestions.push({ value: origin, label: origin });
	}

	return suggestions;
}

export function canaryUpstreamSuggestionsFromManagedEndpointDests(
	endpoints: readonly Pick<
		AdminEndpoint,
		"endpoint_id" | "node_id" | "kind" | "meta"
	>[],
	nodeId: string | null | undefined,
): ManagedVlessAutocompleteSuggestion[] {
	const trimmedNodeId = nodeId?.trim() ?? "";
	if (!trimmedNodeId) return [];

	return canaryUpstreamSuggestionsFromAuthorities(
		endpoints.flatMap((endpoint) => {
			if (
				endpoint.kind !== "vless_reality_vision_tcp" ||
				endpoint.node_id !== trimmedNodeId
			) {
				return [];
			}

			const meta = isRecord(endpoint.meta) ? endpoint.meta : null;
			if (!meta || meta.managed_default !== true) return [];

			const reality = isRecord(meta.reality) ? meta.reality : null;
			return typeof reality?.dest === "string" ? [reality.dest] : [];
		}),
	);
}

export function acceptedAuthoritySuggestionsFromAccessHost(
	accessHost: string | null | undefined,
	port: number | string | null | undefined = 443,
): string[] {
	const trimmed = accessHost?.trim() ?? "";
	if (!trimmed) return [];

	const parsedPort =
		typeof port === "number"
			? Number.isInteger(port) && port > 0 && port <= 65535
				? port
				: null
			: typeof port === "string"
				? (() => {
						const numeric = Number.parseInt(port.trim(), 10);
						return Number.isInteger(numeric) && numeric > 0 && numeric <= 65535
							? numeric
							: null;
					})()
				: null;
	if (parsedPort === null) return [];

	const normalized = normalizeAcceptedAuthority(
		parsedPort === 443 ? trimmed : `${trimmed}:${parsedPort}`,
	);
	if (!normalized || validateAcceptedAuthority(normalized)) return [];

	return normalized.endsWith(":443")
		? [normalized.slice(0, -":443".length)]
		: [normalized];
}
