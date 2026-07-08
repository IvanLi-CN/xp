export type CanaryUpstreamMode = "auto" | "http1" | "h2c";

export type VlessMetaSnapshot = {
	realityDest: string;
	realityServerNames: string[];
	realityServerNamesSource: "manual" | "global";
	realityFingerprint: string;
	managedDefault: boolean;
	canaryUpstreamUrl: string;
	canaryUpstreamMode: CanaryUpstreamMode;
	acceptedAuthorities: string[];
};

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null;
}

function asString(value: unknown): string | undefined {
	return typeof value === "string" ? value : undefined;
}

function asStringArray(value: unknown): string[] | undefined {
	if (!Array.isArray(value)) return undefined;
	const filtered = value.filter((entry) => typeof entry === "string");
	return filtered.length === value.length ? filtered : undefined;
}

function asRealityServerNamesSource(
	value: unknown,
): "manual" | "global" | undefined {
	if (value === "manual" || value === "global") return value;
	return undefined;
}

function asCanaryUpstreamMode(value: unknown): CanaryUpstreamMode | undefined {
	if (value === "auto" || value === "http1" || value === "h2c") return value;
	return undefined;
}

export function parseVlessMeta(
	meta: Record<string, unknown>,
): VlessMetaSnapshot {
	const reality = isRecord(meta.reality) ? meta.reality : undefined;
	const upstream = isRecord(meta.canary_upstream)
		? meta.canary_upstream
		: undefined;
	return {
		realityDest: asString(reality?.dest) ?? "",
		realityServerNames: asStringArray(reality?.server_names) ?? [],
		realityServerNamesSource:
			asRealityServerNamesSource(reality?.server_names_source) ?? "manual",
		realityFingerprint: asString(reality?.fingerprint) ?? "",
		managedDefault: meta.managed_default === true,
		canaryUpstreamUrl: asString(upstream?.url) ?? "",
		canaryUpstreamMode: asCanaryUpstreamMode(upstream?.mode) ?? "auto",
		acceptedAuthorities: asStringArray(meta.accepted_authorities) ?? [],
	};
}

export function routeAuthority(
	hostname: string | undefined,
	port: number,
): string {
	const host = hostname?.trim().replace(/\.$/, "") || "-";
	if (host === "-") return host;
	return port === 443 ? host : `${host}:${port}`;
}

export function arraysEqual(left: string[], right: string[]): boolean {
	if (left.length !== right.length) return false;
	return left.every((value, index) => value === right[index]);
}

export function authoritySetsEqual(left: string[], right: string[]): boolean {
	if (left.length !== right.length) return false;
	const leftSorted = [...left].sort();
	const rightSorted = [...right].sort();
	return leftSorted.every((value, index) => value === rightSorted[index]);
}

export function dedupeAuthorities(values: string[]): string[] {
	return [...new Set(values)];
}
