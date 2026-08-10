import yaml from "js-yaml";

export type MihomoProfileDraft = {
	mixin_yaml: string;
	extra_proxies_yaml: string;
	extra_proxy_providers_yaml: string;
};

const { dump, load } = yaml;

function isYamlMapping(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}

function parseYamlSequenceOrNull(raw: string): unknown[] | null {
	if (raw.trim() === "") return [];
	try {
		const value = load(raw);
		return Array.isArray(value) ? value : null;
	} catch {
		return null;
	}
}

function parseYamlMappingOrNull(raw: string): Record<string, unknown> | null {
	if (raw.trim() === "") return {};
	try {
		const value = load(raw);
		return isYamlMapping(value) ? value : null;
	} catch {
		return null;
	}
}

function getYamlProxyName(proxy: unknown): string | null {
	return isYamlMapping(proxy) && typeof proxy.name === "string"
		? proxy.name
		: null;
}

function mergeLegacyProxiesPreferExtra(
	extraProxies: unknown[],
	legacyProxies: unknown[],
): unknown[] | null {
	const existingNames = new Set<string>();
	for (const proxy of extraProxies) {
		const name = getYamlProxyName(proxy);
		if (!name) return null;
		existingNames.add(name);
	}
	const merged = [...extraProxies];
	for (const proxy of legacyProxies) {
		const name = getYamlProxyName(proxy);
		if (!name) return null;
		if (!existingNames.has(name)) merged.push(proxy);
	}
	return merged;
}

export function normalizeMihomoProfileDraftForSave(
	profile: MihomoProfileDraft,
): MihomoProfileDraft {
	if (profile.mixin_yaml.trim() === "") return profile;
	let mixinRoot: unknown;
	try {
		mixinRoot = load(profile.mixin_yaml);
	} catch {
		return profile;
	}
	if (!isYamlMapping(mixinRoot)) return profile;

	let mixinMap: Record<string, unknown> = { ...mixinRoot };
	let mixinChanged = false;
	let extraProxiesYaml = profile.extra_proxies_yaml;
	let extraProxyProvidersYaml = profile.extra_proxy_providers_yaml;
	if (Object.prototype.hasOwnProperty.call(mixinMap, "proxies")) {
		const value = mixinMap.proxies;
		if (!Array.isArray(value)) return profile;
		const extraProxies = parseYamlSequenceOrNull(extraProxiesYaml);
		if (extraProxies === null) return profile;
		const merged = mergeLegacyProxiesPreferExtra(extraProxies, value);
		if (merged === null) return profile;
		extraProxiesYaml = dump(merged);
		const { proxies: _removedProxies, ...nextMixinMap } = mixinMap;
		mixinMap = nextMixinMap;
		mixinChanged = true;
	}
	if (Object.prototype.hasOwnProperty.call(mixinMap, "proxy-providers")) {
		const value = mixinMap["proxy-providers"];
		if (!isYamlMapping(value)) return profile;
		const extraProxyProviders = parseYamlMappingOrNull(extraProxyProvidersYaml);
		if (extraProxyProviders === null) return profile;
		extraProxyProvidersYaml = dump({ ...value, ...extraProxyProviders });
		const { "proxy-providers": _removedProxyProviders, ...nextMixinMap } =
			mixinMap;
		mixinMap = nextMixinMap;
		mixinChanged = true;
	}
	return mixinChanged
		? {
				mixin_yaml: dump(mixinMap),
				extra_proxies_yaml: extraProxiesYaml,
				extra_proxy_providers_yaml: extraProxyProvidersYaml,
			}
		: profile;
}
