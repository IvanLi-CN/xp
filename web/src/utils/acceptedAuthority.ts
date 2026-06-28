function normalizeAuthorityHost(value: string): string {
	return value.trim().replace(/\.$/, "").toLowerCase();
}

export function normalizeAcceptedAuthority(value: string): string {
	const trimmed = value.trim();
	if (!trimmed) return "";

	if (trimmed.startsWith("[")) {
		const end = trimmed.indexOf("]");
		if (end === -1) return trimmed;
		const host = normalizeAuthorityHost(trimmed.slice(1, end));
		const rest = trimmed.slice(end + 1);
		return `[${host}]${rest}`;
	}

	const splitIndex = trimmed.lastIndexOf(":");
	if (splitIndex <= 0) return trimmed;
	const host = trimmed.slice(0, splitIndex);
	const port = trimmed.slice(splitIndex + 1);
	if (host.includes(":")) return trimmed;
	return `${normalizeAuthorityHost(host)}:${port}`;
}

export function validateAcceptedAuthority(value: string): string | null {
	const trimmed = value.trim();
	if (!trimmed) return "accepted host:port is required.";
	if (/\s/.test(trimmed)) return "accepted host:port must not contain spaces.";
	if (trimmed.includes("://"))
		return "accepted host:port must not include scheme (://).";
	if (trimmed.includes("/"))
		return "accepted host:port must not include path (/).";
	if (trimmed.includes("?"))
		return "accepted host:port must not include query (?).";
	if (trimmed.includes("#"))
		return "accepted host:port must not include fragment (#).";

	let host = "";
	let port = "";
	if (trimmed.startsWith("[")) {
		const end = trimmed.indexOf("]");
		if (end === -1) return "IPv6 host must use [addr]:port.";
		host = trimmed.slice(1, end);
		const rest = trimmed.slice(end + 1);
		if (!rest.startsWith(":")) {
			return "accepted host:port must include port (:).";
		}
		port = rest.slice(1);
		if (!host.includes(":")) return "Bracketed host must be IPv6.";
	} else {
		const splitIndex = trimmed.lastIndexOf(":");
		if (splitIndex <= 0) return "accepted host:port must include port (:).";
		host = trimmed.slice(0, splitIndex);
		port = trimmed.slice(splitIndex + 1);
		if (host.includes(":")) return "IPv6 host must use [addr]:port.";
	}

	if (!port) return "accepted host:port must include port (:).";
	if (!/^\d+$/.test(port)) return "accepted host:port port must be 1..65535.";
	const parsedPort = Number.parseInt(port, 10);
	if (!Number.isInteger(parsedPort) || parsedPort < 1 || parsedPort > 65535) {
		return "accepted host:port port must be 1..65535.";
	}

	if (/^\d{1,3}(?:\.\d{1,3}){3}$/.test(host)) {
		const octets = host.split(".").map((part) => Number.parseInt(part, 10));
		if (octets.some((part) => Number.isNaN(part) || part < 0 || part > 255)) {
			return "accepted host:port must use a valid IPv4, hostname, or bracketed IPv6.";
		}
		return null;
	}

	if (host.includes(":")) {
		if (!/^[0-9a-fA-F:]+$/.test(host)) {
			return "accepted host:port must use a valid IPv4, hostname, or bracketed IPv6.";
		}
		return null;
	}

	const normalizedHost = normalizeAuthorityHost(host);
	if (normalizedHost.length > 253) {
		return "accepted host:port hostname is too long.";
	}
	if (normalizedHost.startsWith(".") || normalizedHost.endsWith(".")) {
		return "accepted host:port hostname must not start or end with a dot.";
	}
	if (normalizedHost.includes("..")) {
		return "accepted host:port hostname must not contain consecutive dots.";
	}
	if (!/^[0-9A-Za-z.-]+$/.test(normalizedHost)) {
		return "accepted host:port must use a valid IPv4, hostname, or bracketed IPv6.";
	}
	const labels = normalizedHost.split(".");
	if (labels.length < 2) {
		return "accepted host:port hostname must contain at least one dot.";
	}
	const tld = labels[labels.length - 1] ?? "";
	if (tld.length < 2) return "accepted host:port hostname TLD is too short.";
	for (const label of labels) {
		if (!label) return "accepted host:port hostname contains an empty label.";
		if (label.length > 63) {
			return "accepted host:port hostname label is too long.";
		}
		if (!/^[0-9A-Za-z](?:[0-9A-Za-z-]*[0-9A-Za-z])?$/.test(label)) {
			return "accepted host:port hostname labels must start/end with letter or digit.";
		}
	}

	return null;
}
