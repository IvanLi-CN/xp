import type {
	DemoEndpoint,
	DemoEndpointStatus,
	DemoNodeStatus,
	DemoUserStatus,
} from "./types";

export function formatGb(value: number | null): string {
	if (value === null) return "unlimited";
	return `${value.toLocaleString()} GiB`;
}

export function formatPercent(used: number, limit: number | null): string {
	if (limit === null || limit <= 0) return "unlimited";
	return `${Math.min(100, Math.round((used / limit) * 100))}%`;
}

export function endpointKindLabel(kind: DemoEndpoint["kind"]): string {
	if (kind === "vless_reality_vision_tcp") return "VLESS Reality Vision TCP";
	return "SS2022 BLAKE3 AES-128-GCM";
}

export function nodeStatusVariant(status: DemoNodeStatus) {
	if (status === "healthy") return "success" as const;
	if (status === "degraded") return "warning" as const;
	return "destructive" as const;
}

export function endpointStatusVariant(status: DemoEndpointStatus) {
	if (status === "serving") return "success" as const;
	if (status === "degraded") return "warning" as const;
	return "destructive" as const;
}

export function userStatusVariant(status: DemoUserStatus) {
	if (status === "active") return "success" as const;
	if (status === "quota_limited") return "warning" as const;
	return "ghost" as const;
}

export function shortDate(value: string | null): string {
	if (!value) return "never";
	return new Intl.DateTimeFormat("en", {
		month: "short",
		day: "2-digit",
		hour: "2-digit",
		minute: "2-digit",
	}).format(new Date(value));
}

export function subscriptionUrl(token: string): string {
	return `https://xp.example.net/api/sub/${encodeURIComponent(token)}?format=mihomo`;
}
