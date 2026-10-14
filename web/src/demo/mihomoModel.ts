import yaml from "js-yaml";

import {
	errorResponse,
	textResponse,
} from "../../.storybook/mocks/mockResponses";
import type { AdminUserMihomoProfile } from "../api/adminUsers";

function mapping(value: unknown): Record<string, unknown> {
	if (value === undefined || value === null) return {};
	if (typeof value !== "object" || Array.isArray(value))
		throw new Error("Expected a YAML mapping.");
	return value as Record<string, unknown>;
}

export function renderDemoMihomoSubscription(
	profile: AdminUserMihomoProfile | undefined,
	requestUrl: URL,
): Response {
	const mirror = requestUrl.searchParams.get("external_resources") === "mirror";
	if (
		mirror &&
		(!profile || Object.values(profile).every((value) => !value.trim()))
	) {
		return errorResponse(
			422,
			"invalid_request",
			"Mihomo mirror requires a saved Mihomo profile.",
		);
	}
	try {
		const mixin = mapping(yaml.load(profile?.mixin_yaml ?? ""));
		const providers = mapping(
			yaml.load(profile?.extra_proxy_providers_yaml ?? ""),
		);
		const proxies = yaml.load(profile?.extra_proxies_yaml ?? "") ?? [];
		if (!Array.isArray(proxies))
			throw new Error("Expected extra proxies to be a YAML sequence.");
		const output: Record<string, unknown> = {
			...mixin,
			proxies,
			"proxy-providers": {
				...providers,
				system: {
					type: "http",
					url: new URL(
						`${requestUrl.pathname}/mihomo/provider/system`,
						requestUrl,
					).href,
				},
			},
		};
		if (mirror) {
			const rewrite = (raw: unknown): string => {
				if (typeof raw !== "string")
					throw new Error("External resource URL must be HTTPS.");
				const url = new URL(raw);
				if (url.protocol !== "https:" || url.username || url.password) {
					throw new Error(
						"External resource URL must be HTTPS without userinfo.",
					);
				}
				let hash = 2166136261;
				for (const character of url.href)
					hash = Math.imul(hash ^ character.charCodeAt(0), 16777619);
				return new URL(
					`/api/mihomo/resources/demo-${(hash >>> 0).toString(16)}`,
					requestUrl,
				).href;
			};
			const geox = {
				geoip: "https://assets.example.net/geoip.dat",
				geosite: "https://assets.example.net/geosite.dat",
				mmdb: "https://assets.example.net/country.mmdb",
				asn: "https://assets.example.net/asn.mmdb",
				...mapping(output["geox-url"]),
			};
			output["geox-url"] = Object.fromEntries(
				Object.entries(geox).map(([key, value]) => [key, rewrite(value)]),
			);
			for (const key of ["rule-providers", "proxy-providers"]) {
				const entries = mapping(output[key]);
				for (const [name, raw] of Object.entries(entries)) {
					if (key === "proxy-providers" && name === "system") continue;
					const provider = mapping(raw);
					if (provider.type !== "http") continue;
					if (provider.headers || provider.header)
						throw new Error("Custom resource headers cannot be mirrored.");
					entries[name] = {
						...provider,
						url: rewrite(provider.url),
						proxy: "DIRECT",
					};
				}
				output[key] = entries;
			}
		}
		return textResponse(
			`# Simulated Mihomo subscription${mirror ? " · XP mirror mode" : ""}\n${yaml.dump(output)}`,
		);
	} catch (error) {
		return errorResponse(
			422,
			"invalid_request",
			error instanceof Error ? error.message : "Invalid Mihomo profile.",
		);
	}
}
