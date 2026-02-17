type RealityDomainLike = {
	server_name: string;
	disabled_node_ids?: string[];
};

// Keep behavior aligned with backend's `derive_global_reality_server_names`.
export function deriveGlobalRealityServerNames(
	domains: RealityDomainLike[],
	nodeId: string,
): string[] {
	const out: string[] = [];
	const seen = new Set<string>();

	for (const domain of domains) {
		const disabled = domain.disabled_node_ids ?? [];
		if (disabled.includes(nodeId)) continue;

		const trimmed = domain.server_name.trim();
		if (!trimmed) continue;

		const key = trimmed.toLowerCase();
		if (seen.has(key)) continue;
		seen.add(key);

		out.push(trimmed);
	}

	return out;
}
