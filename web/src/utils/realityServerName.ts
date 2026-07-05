export function normalizeRealityServerName(value: string): string {
	return value.trim();
}

export function realityServerNameSuggestionFromDest(
	dest: string,
): string | null {
	const candidate = normalizeRealityServerName(dest);
	if (validateRealityServerName(candidate)) return null;
	return candidate;
}

export function validateRealityServerName(value: string): string | null {
	const trimmed = normalizeRealityServerName(value);
	if (!trimmed) return "serverName is required.";
	if (/\s/.test(trimmed)) return "serverName must not contain spaces.";

	if (trimmed.includes("://"))
		return "serverName must not include scheme (://).";
	if (trimmed.includes("/")) return "serverName must not include path (/).";
	if (trimmed.includes("*")) return "Wildcard is not supported.";

	const { host, port } = splitHostPort(trimmed);
	if (!host) return "serverName host is required.";
	if (port !== null) {
		if (!/^\d+$/.test(port)) return "serverName port must be 1..65535.";
		const parsedPort = Number.parseInt(port, 10);
		if (!Number.isInteger(parsedPort) || parsedPort < 1 || parsedPort > 65535) {
			return "serverName port must be 1..65535.";
		}
	}

	// RFC 1035 / RFC 1123-ish hostname rules (ASCII only).
	if (host.length > 253) return "serverName hostname is too long (max 253).";
	if (host.startsWith(".") || host.endsWith(".")) {
		return "serverName must not start or end with a dot (.).";
	}
	if (host.includes("..")) {
		return "serverName must not contain consecutive dots ('..').";
	}
	if (!/^[0-9A-Za-z.-]+$/.test(host)) {
		return "serverName must be a valid hostname (letters/digits/dots/hyphens).";
	}

	const labels = host.split(".");
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

function splitHostPort(value: string): { host: string; port: string | null } {
	const splitIndex = value.lastIndexOf(":");
	if (splitIndex <= 0) return { host: value, port: null };
	const host = value.slice(0, splitIndex);
	if (host.includes(":")) return { host: value, port: null };
	return { host, port: value.slice(splitIndex + 1) };
}
