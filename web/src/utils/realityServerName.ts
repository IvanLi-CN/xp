export function normalizeRealityServerName(value: string): string {
	return value.trim();
}

// Validation for VLESS REALITY "server_name" / TLS SNI.
//
// We keep it intentionally strict: only a plain hostname is allowed.
// This prevents accidentally pasting a full URL or "host:port" which would
// later produce broken client configs.
export function validateRealityServerName(value: string): string | null {
	const trimmed = normalizeRealityServerName(value);
	if (!trimmed) return "serverName is required.";
	if (/\s/.test(trimmed)) return "serverName must not contain spaces.";

	// Common user mistakes: pasting a URL or "host/path" or "host:port".
	if (trimmed.includes("://"))
		return "serverName must not include scheme (://).";
	if (trimmed.includes("/")) return "serverName must not include path (/).";
	if (trimmed.includes(":")) return "serverName must not include port (:).";
	if (trimmed.includes("*")) return "Wildcard is not supported.";

	// RFC 1035 / RFC 1123-ish hostname rules (ASCII only).
	if (trimmed.length > 253) return "serverName is too long (max 253).";
	if (trimmed.startsWith(".") || trimmed.endsWith(".")) {
		return "serverName must not start or end with a dot (.).";
	}
	if (trimmed.includes("..")) {
		return "serverName must not contain consecutive dots ('..').";
	}
	if (!/^[0-9A-Za-z.-]+$/.test(trimmed)) {
		return "serverName must be a valid hostname (letters/digits/dots/hyphens).";
	}

	const labels = trimmed.split(".");
	if (labels.length < 2) {
		return "serverName must contain at least one dot (example.com).";
	}

	// Heuristic: public TLDs are at least 2 chars today; this blocks obvious typos like "cc.c".
	// If you need internal single-letter TLDs, relax this rule.
	const tld = labels[labels.length - 1] ?? "";
	if (tld.length < 2) return "serverName TLD is too short (min 2).";

	for (const label of labels) {
		if (label.length === 0) return "serverName contains an empty label.";
		if (label.length > 63) return "serverName label is too long (max 63).";
		if (!/^[0-9A-Za-z](?:[0-9A-Za-z-]*[0-9A-Za-z])?$/.test(label)) {
			return "serverName labels must start/end with letter or digit; '-' allowed in the middle.";
		}
	}

	return null;
}
