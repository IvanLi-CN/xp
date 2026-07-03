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

function defaultPortForProtocol(protocol: string): string | null {
	if (protocol === "https:") return "443";
	if (protocol === "http:") return "80";
	return null;
}
