import {
	type MihomoSmuxConfig,
	parseMihomoSmuxConfig,
} from "../api/adminEndpoints";

export function buildMihomoSmuxConfig(
	config: MihomoSmuxConfig,
	maxConnectionsValue: string,
	minStreamsValue: string,
): MihomoSmuxConfig {
	const maxConnections = Number.parseInt(maxConnectionsValue, 10);
	if (
		!Number.isFinite(maxConnections) ||
		maxConnections < 1 ||
		maxConnections > 16
	) {
		throw new Error("Maximum physical connections must be between 1 and 16.");
	}
	const minStreams = Number.parseInt(minStreamsValue, 10);
	if (!Number.isFinite(minStreams) || minStreams < 1 || minStreams > 64) {
		throw new Error("Minimum streams must be between 1 and 64.");
	}
	return {
		...config,
		max_connections: maxConnections,
		min_streams: minStreams,
	};
}

export function mihomoSmuxConfigsEqual(
	left: MihomoSmuxConfig,
	right: MihomoSmuxConfig,
): boolean {
	return (
		left.enabled === right.enabled &&
		left.max_connections === right.max_connections &&
		left.min_streams === right.min_streams &&
		left.only_tcp === right.only_tcp
	);
}

export function changedMihomoSmuxConfig(
	available: boolean,
	current: unknown,
	config: MihomoSmuxConfig,
	maxConnections: string,
	minStreams: string,
): MihomoSmuxConfig | undefined {
	if (!available) return undefined;
	const next = buildMihomoSmuxConfig(config, maxConnections, minStreams);
	return mihomoSmuxConfigsEqual(next, parseMihomoSmuxConfig(current))
		? undefined
		: next;
}
