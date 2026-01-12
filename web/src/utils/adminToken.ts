export type AdminTokenParseResult =
	| { token: string }
	| { error: string; token?: undefined };

export function parseAdminTokenInput(raw: string): AdminTokenParseResult {
	const trimmed = raw.trim();
	if (!trimmed) return { error: "Token is required." };

	if (/^Bearer\s+/i.test(trimmed)) {
		return { error: "Paste the token only (without the 'Bearer ' prefix)." };
	}

	if (/XP_ADMIN_TOKEN\s*=|^export\s+/i.test(trimmed)) {
		return { error: "Paste the token only (not an env var line)." };
	}

	if (/\s/.test(trimmed)) {
		return { error: "Token must not contain whitespace." };
	}

	return { token: trimmed };
}
