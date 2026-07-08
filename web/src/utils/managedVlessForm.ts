import {
	normalizeAcceptedAuthority,
	validateAcceptedAuthority,
} from "./acceptedAuthority";

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
