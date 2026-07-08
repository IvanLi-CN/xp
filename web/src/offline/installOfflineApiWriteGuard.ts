const MUTATING_METHODS = new Set(["POST", "PUT", "PATCH", "DELETE"]);

let installed = false;

function isAdminApiRequest(urlLike: string): boolean {
	try {
		const url = new URL(urlLike, window.location.origin);
		return (
			url.origin === window.location.origin && url.pathname.startsWith("/api/")
		);
	} catch {
		return false;
	}
}

export function installOfflineApiWriteGuard() {
	if (installed || typeof window === "undefined") return;
	installed = true;

	const originalFetch = window.fetch.bind(window);

	window.fetch = async (input, init) => {
		const request = new Request(input, init);
		if (
			typeof navigator !== "undefined" &&
			navigator.onLine === false &&
			MUTATING_METHODS.has(request.method.toUpperCase()) &&
			isAdminApiRequest(request.url)
		) {
			throw new Error(
				"Offline read-only mode is active. Reconnect before changing data.",
			);
		}
		return originalFetch(input, init);
	};
}
