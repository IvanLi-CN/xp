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

export function canaryUpstreamSuggestionsFromApiBaseUrl(
	apiBaseUrl: string | null | undefined,
): ManagedVlessAutocompleteSuggestion[] {
	const trimmed = apiBaseUrl?.trim() ?? "";
	if (!trimmed) return [];

	let url: URL;
	try {
		url = new URL(trimmed);
	} catch {
		return [];
	}

	if (
		(url.protocol !== "http:" && url.protocol !== "https:") ||
		!url.hostname
	) {
		return [];
	}

	return [{ value: url.origin, label: url.origin }];
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
