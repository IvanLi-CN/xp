import type { AdminNode } from "../../src/api/adminNodes";
import type { AdminNodeTcpConnectionsResponse } from "../../src/api/adminTcpConnections";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

const STEP_MS = 5 * 60_000;

/**
 * Deterministic full-site payload for the demo and Storybook mocks: one day of 5-minute buckets
 * with a broad daily rhythm, split into the four connection classes.
 */
export function buildNodeTcpConnections(
	node: AdminNode,
): AdminNodeTcpConnectionsResponse {
	const endpointA = {
		endpoint_id: fixtureCatalog.endpointId.fixture27(),
		endpoint_tag: fixtureCatalog.endpointTag.fixture49(),
		port: fixtureCatalog.endpoint.port443(),
	};
	const endpointB = {
		endpoint_id: fixtureCatalog.endpointId.fixture50(),
		endpoint_tag: fixtureCatalog.endpointTag.fixture51(),
		port: fixtureCatalog.endpoint.port8443(),
	};
	const windowStart = Date.parse(fixtureCatalog.timestamp.t20260307T010000());
	const windowEnd = Date.parse(fixtureCatalog.timestamp.t20260308T005900());
	const bucketCount = Math.floor((windowEnd - windowStart) / STEP_MS) + 1;
	const minuteCount = Math.floor((windowEnd - windowStart) / 60_000) + 1;
	const wave = (index: number) => {
		const phase = (index / bucketCount) * Math.PI * 4;
		return Math.max(
			0,
			Math.round(8 + 5 * Math.sin(phase) + 2 * Math.sin(phase / 3)),
		);
	};
	const point = (bucket: number, value: number) => ({
		bucket_start: new Date(bucket).toISOString(),
		min: value,
		avg: value,
		max: value,
		samples_expected: 5,
		samples_actual: 5,
		truncated: false,
		partial: false,
	});
	const totalPoints = Array.from({ length: bucketCount }, (_, index) =>
		point(windowStart + index * STEP_MS, wave(index)),
	);
	const scale = (factor: number) =>
		totalPoints.map((entry) => ({
			...entry,
			min: Math.round(entry.min * factor),
			avg: Math.round(entry.avg * factor),
			max: Math.round(entry.max * factor),
		}));
	const minutes = Array.from({ length: minuteCount }, (_, index) =>
		new Date(windowStart + index * 60_000).toISOString(),
	);
	return {
		node,
		window: "24h",
		window_start: fixtureCatalog.timestamp.t20260307T010000(),
		window_end: fixtureCatalog.timestamp.t20260308T005900(),
		warnings: [],
		endpoints: [endpointA, endpointB],
		per_endpoint_series: [
			{
				...endpointA,
				series: minutes.map((minute, index) => ({
					minute,
					count: Math.max(0, Math.round(4 + 3 * Math.sin(index / 17))),
				})),
			},
			{
				...endpointB,
				series: minutes.map((minute, index) => ({
					minute,
					count: Math.max(0, Math.round(2 + 2 * Math.cos(index / 13))),
				})),
			},
		],
		quality: "complete",
		reasons: [],
		truncated: false,
		truncated_buckets: [],
		partial_buckets: [],
		deleted_subjects: [],
		subjects_truncated: false,
		subjects_omitted: false,
		subjects_next_cursor: null,
		resolution: "1m",
		watermark: fixtureCatalog.timestamp.t20260308T005900(),
		gaps: [],
		total: { approximate: false, points: totalPoints },
		classes: [
			{ class: "inbound", approximate: false, points: scale(0.55) },
			{ class: "control_plane", approximate: false, points: scale(0.25) },
			{ class: "outbound_runtime", approximate: false, points: scale(0.15) },
			{ class: "outbound_other", approximate: false, points: scale(0.05) },
		],
		subjects: [endpointA, endpointB].map((endpoint) => ({
			class: "inbound",
			subject: endpoint.endpoint_id,
			approximate: false,
			points: totalPoints,
		})),
	};
}
