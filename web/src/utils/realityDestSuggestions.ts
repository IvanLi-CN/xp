export function realityDestFromApiBaseUrl(apiBaseUrl: string): string | null {
	const trimmed = apiBaseUrl.trim();
	if (!trimmed) return null;

	let url: URL;
	try {
		url = new URL(trimmed);
	} catch {
		return null;
	}

	if (!url.hostname) return null;
	if (url.port) return url.host;

	const defaultPort = defaultPortForProtocol(url.protocol);
	if (!defaultPort) return null;
	return `${url.host}:${defaultPort}`;
}

export function realityDestFromBindAddress(bindAddress: string): string | null {
	const trimmed = bindAddress.trim();
	if (!trimmed) return null;

	const bracketedIpv6Match = trimmed.match(/^\[([^\]]+)\]:(\d+)$/);
	if (bracketedIpv6Match) {
		const [, host, port] = bracketedIpv6Match;
		if (host && port) return `[${host}]:${port}`;
	}

	const lastColonIndex = trimmed.lastIndexOf(":");
	if (lastColonIndex <= 0 || lastColonIndex === trimmed.length - 1) {
		return null;
	}

	const host = trimmed.slice(0, lastColonIndex);
	const port = trimmed.slice(lastColonIndex + 1);
	if (!/^\d+$/.test(port)) return null;
	if (host.includes(":")) return null;
	return `${host}:${port}`;
}

function defaultPortForProtocol(protocol: string): string | null {
	if (protocol === "https:") return "443";
	if (protocol === "http:") return "80";
	return null;
}
