import { isBackendApiError } from "../api/backendError";

export const FRAMEWORK_ERROR_CATEGORIES = [
	"chunk-load",
	"cache-mismatch",
	"offline",
	"api-incompatibility",
	"react-runtime",
	"unknown",
] as const;

export type FrameworkErrorCategory =
	(typeof FRAMEWORK_ERROR_CATEGORIES)[number];

type ClassifyOptions = {
	isOnline?: boolean;
};

type CacheStorageLike = Pick<CacheStorage, "keys" | "delete">;

export type CacheRecoveryResult =
	| { status: "cleared"; deleted: string[] }
	| {
			status: "skipped";
			reason:
				| "offline"
				| "replacement-unavailable"
				| "service-worker-unavailable"
				| "cache-unavailable";
			deleted: string[];
	  };

type CacheRecoveryOptions = {
	cacheStorage?: CacheStorageLike;
	origin?: string;
	isOnline?: boolean;
	replacementReady?: () => Promise<boolean>;
};

type ServiceWorkerRegistrationLike = {
	update: () => Promise<unknown>;
	waiting: unknown | null;
};

type SafeCacheRecoveryOptions = CacheRecoveryOptions & {
	buildId?: string;
	serviceWorkerContainer?: {
		getRegistration: () => Promise<ServiceWorkerRegistrationLike | undefined>;
		controller?: { postMessage: (message: unknown) => void } | null;
		addEventListener?: (
			type: "message",
			listener: (event: MessageEvent) => void,
		) => void;
		removeEventListener?: (
			type: "message",
			listener: (event: MessageEvent) => void,
		) => void;
	};
	replacementProbe?: (
		registration: ServiceWorkerRegistrationLike,
	) => Promise<boolean>;
};

type DiagnosticInput = {
	error: unknown;
	category: FrameworkErrorCategory;
	buildId?: string;
	path?: string;
	isOnline?: boolean;
	repeatFailure?: boolean;
};

function getErrorProperty(error: unknown, property: string): unknown {
	if (!error || typeof error !== "object") return undefined;
	return (error as Record<string, unknown>)[property];
}

function getErrorText(error: unknown): string {
	if (error instanceof Error) {
		return [error.name, error.message, error.stack].filter(Boolean).join("\n");
	}
	if (typeof error === "string") return error;
	try {
		return JSON.stringify(error);
	} catch {
		return String(error);
	}
}

function isOffline(options: ClassifyOptions): boolean {
	if (options.isOnline === false) return true;
	return typeof navigator !== "undefined" && navigator.onLine === false;
}

export function classifyFrameworkError(
	error: unknown,
	options: ClassifyOptions = {},
): FrameworkErrorCategory {
	if (isOffline(options)) return "offline";

	const message = getErrorText(error);
	const normalizedMessage = message.toLowerCase();
	const code = String(getErrorProperty(error, "code") ?? "").toLowerCase();
	const name = String(getErrorProperty(error, "name") ?? "").toLowerCase();

	if (
		code.includes("cache_version") ||
		code.includes("cache-mismatch") ||
		name.includes("cacheversion") ||
		/cache\s+(?:version\s+)?mismatch|version\s+mismatch|mixed\s+build/i.test(
			normalizedMessage,
		)
	) {
		return "cache-mismatch";
	}

	const chunkLoadPattern = new RegExp(
		[
			"chunkloaderror",
			"loading chunk",
			"failed to fetch dynamically imported module",
			"importing a module script failed",
			"module script failed",
		].join("|"),
		"i",
	);
	if (chunkLoadPattern.test(normalizedMessage)) {
		return "chunk-load";
	}

	const apiIncompatibilityPattern = new RegExp(
		[
			"api\\s+(?:is\\s+)?incompatib",
			"incompatib(?:le|ility)\\s+(?:api|server)",
			"unsupported\\s+api",
			"missing\\s+(?:api\\s+)?capabilit",
		].join("|"),
		"i",
	);
	if (
		(isBackendApiError(error) &&
			(code.includes("incompat") ||
				code.includes("compatibility") ||
				code.includes("unsupported_api"))) ||
		apiIncompatibilityPattern.test(normalizedMessage)
	) {
		return "api-incompatibility";
	}

	if (
		/react\s+(?:error|invariant)|minified react error|hydration failed/i.test(
			normalizedMessage,
		)
	) {
		return "react-runtime";
	}

	if (
		/failed to fetch|networkerror|network request failed|load failed/i.test(
			normalizedMessage,
		) &&
		isOffline(options)
	) {
		return "offline";
	}

	return "unknown";
}

function redactSensitiveText(value: string): string {
	return value
		.replace(/(authorization\s*:\s*bearer\s+)[^\s,]+/gi, "$1[REDACTED]")
		.replace(
			/([?&](?:token|login_token|access_token|api_key|apikey|secret|password|key)=)[^&#\s]+/gi,
			"$1[REDACTED]",
		)
		.replace(
			/((?:token|login_token|access_token|api_key|apikey|secret|password|key)\s*[:=]\s*)[^\s,]+/gi,
			"$1[REDACTED]",
		)
		.replace(
			/\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b/g,
			"[REDACTED]",
		)
		.replace(/\bBearer\s+[^\s,]+/gi, "Bearer [REDACTED]");
}

function diagnosticPath(path: string | undefined): string {
	if (!path) {
		return typeof window !== "undefined" ? window.location.pathname : "/";
	}
	try {
		return new URL(path, "http://xp.invalid").pathname;
	} catch {
		return path.split(/[?#]/, 1)[0] || "/";
	}
}

export function createDiagnosticDetails(input: DiagnosticInput): string {
	const errorText = redactSensitiveText(getErrorText(input.error)).slice(
		0,
		4000,
	);
	const online =
		input.isOnline ??
		(typeof navigator === "undefined" || navigator.onLine !== false);
	return [
		"XP web runtime diagnostic",
		`build: ${redactSensitiveText(input.buildId ?? "unknown")}`,
		`category: ${input.category}`,
		`path: ${diagnosticPath(input.path)}`,
		`online: ${online ? "true" : "false"}`,
		`repeatFailure: ${input.repeatFailure === true ? "true" : "false"}`,
		"error:",
		errorText || "unknown error",
	].join("\n");
}

function currentOrigin(): string {
	return typeof location === "undefined" ? "" : location.origin;
}

export function isXpAppShellCacheName(
	cacheName: string,
	origin = currentOrigin(),
): boolean {
	const normalizedOrigin = origin.replace(/\/$/, "");
	return (
		cacheName.startsWith("xp-app-shell-") ||
		cacheName === `workbox-precache-v2-${normalizedOrigin}` ||
		cacheName.startsWith(`workbox-precache-v2-${normalizedOrigin}-`)
	);
}

function getCacheStorage(): CacheStorageLike | undefined {
	return typeof caches === "undefined" ? undefined : caches;
}

export async function clearXpAppShellCaches(
	options: CacheRecoveryOptions = {},
): Promise<CacheRecoveryResult> {
	if (options.isOnline === false) {
		return { status: "skipped", reason: "offline", deleted: [] };
	}

	if (options.replacementReady && !(await options.replacementReady())) {
		return {
			status: "skipped",
			reason: "replacement-unavailable",
			deleted: [],
		};
	}

	const cacheStorage = options.cacheStorage ?? getCacheStorage();
	if (!cacheStorage) {
		return { status: "skipped", reason: "cache-unavailable", deleted: [] };
	}

	try {
		const appShellCaches = (await cacheStorage.keys()).filter((cacheName) =>
			isXpAppShellCacheName(cacheName, options.origin),
		);
		const deleted: string[] = [];
		for (const cacheName of appShellCaches) {
			if (await cacheStorage.delete(cacheName)) deleted.push(cacheName);
		}
		return { status: "cleared", deleted };
	} catch {
		return { status: "skipped", reason: "cache-unavailable", deleted: [] };
	}
}

async function defaultReplacementProbe(
	registration: ServiceWorkerRegistrationLike,
): Promise<boolean> {
	// Workbox reaches waiting only after the complete precache install succeeds.
	return registration.waiting !== null;
}

export async function recoverXpAppShell(
	options: SafeCacheRecoveryOptions = {},
): Promise<CacheRecoveryResult> {
	const online =
		options.isOnline ??
		(typeof navigator === "undefined" || navigator.onLine !== false);
	if (!online) {
		return { status: "skipped", reason: "offline", deleted: [] };
	}

	const serviceWorkerContainer =
		options.serviceWorkerContainer ??
		(typeof navigator !== "undefined" && "serviceWorker" in navigator
			? navigator.serviceWorker
			: undefined);
	if (!serviceWorkerContainer) {
		return {
			status: "skipped",
			reason: "service-worker-unavailable",
			deleted: [],
		};
	}

	try {
		const registration = await serviceWorkerContainer.getRegistration();
		if (!registration) {
			return {
				status: "skipped",
				reason: "service-worker-unavailable",
				deleted: [],
			};
		}
		await registration.update();
		const replacementReady = await (
			options.replacementProbe ?? defaultReplacementProbe
		)(registration);
		if (!replacementReady) {
			return {
				status: "skipped",
				reason: "replacement-unavailable",
				deleted: [],
			};
		}

		const controller = serviceWorkerContainer.controller;
		const addEventListener = serviceWorkerContainer.addEventListener as (
			type: "message",
			listener: (event: MessageEvent) => void,
		) => void;
		const removeEventListener = serviceWorkerContainer.removeEventListener as (
			type: "message",
			listener: (event: MessageEvent) => void,
		) => void;
		if (!controller || !addEventListener || !removeEventListener) {
			return {
				status: "skipped",
				reason: "service-worker-unavailable",
				deleted: [],
			};
		}

		return await new Promise<CacheRecoveryResult>((resolve) => {
			const buildId = options.buildId ?? "development";
			const onMessage = (event: MessageEvent) => {
				const data = event.data as
					| {
							type?: string;
							buildId?: string;
							deleted?: unknown;
					  }
					| undefined;
				if (
					data?.buildId !== buildId ||
					(data.type !== "XP_CACHE_RECOVERY_READY" &&
						data.type !== "XP_CACHE_RECOVERY_UNAVAILABLE")
				) {
					return;
				}
				clearTimeout(timeoutId);
				removeEventListener("message", onMessage);
				const deleted = Array.isArray(data.deleted)
					? data.deleted.filter(
							(value): value is string => typeof value === "string",
						)
					: [];
				resolve(
					data.type === "XP_CACHE_RECOVERY_READY"
						? { status: "cleared", deleted }
						: {
								status: "skipped",
								reason: "replacement-unavailable",
								deleted: [],
							},
				);
			};
			const timeoutId = setTimeout(() => {
				removeEventListener("message", onMessage);
				resolve({
					status: "skipped",
					reason: "replacement-unavailable",
					deleted: [],
				});
			}, 10_000);
			addEventListener("message", onMessage);
			controller.postMessage({
				type: "XP_REQUEST_CACHE_RECOVERY",
				buildId,
			});
		});
	} catch {
		return {
			status: "skipped",
			reason: "replacement-unavailable",
			deleted: [],
		};
	}
}
