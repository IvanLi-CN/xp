import type { AdminNode } from "../../src/api/adminNodes";
import type { AdminNodeTcpConnectionsResponse } from "../../src/api/adminTcpConnections";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

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
	// Bucket labels come from approved catalog timestamps; the demo never invents minute data.
	const bucketLabels = [
		fixtureCatalog.timestamp.t20260308T000000(),
		fixtureCatalog.timestamp.t20260308T000100(),
		fixtureCatalog.timestamp.t20260308T000200(),
		fixtureCatalog.timestamp.t20260308T003000(),
		fixtureCatalog.timestamp.t20260308T005500(),
		fixtureCatalog.timestamp.t20260308T005800(),
		fixtureCatalog.timestamp.t20260308T005900(),
	];
	const wave = [3, 9, 12, 14, 9, 5, 6];
	const point = (bucket: string, value: number) => ({
		bucket_start: bucket,
		min: value,
		avg: value,
		max: value,
		samples_expected: 5,
		samples_actual: 5,
		truncated: false,
		partial: false,
	});
	const totalPoints = bucketLabels.map((bucket, index) =>
		point(bucket, wave[index]),
	);
	const scale = (factor: number) =>
		totalPoints.map((entry) => ({
			...entry,
			min: Math.round(entry.min * factor),
			avg: Math.round(entry.avg * factor),
			max: Math.round(entry.max * factor),
		}));
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
				series: [
					{ minute: fixtureCatalog.timestamp.t20260308T000000(), count: 4 },
					{ minute: fixtureCatalog.timestamp.t20260308T000100(), count: 6 },
					{ minute: fixtureCatalog.timestamp.t20260308T000200(), count: 5 },
					{ minute: fixtureCatalog.timestamp.t20260308T003000(), count: 3 },
					{ minute: fixtureCatalog.timestamp.t20260308T005500(), count: 2 },
					{ minute: fixtureCatalog.timestamp.t20260308T005800(), count: 2 },
					{ minute: fixtureCatalog.timestamp.t20260308T005900(), count: 3 },
				],
			},
			{
				...endpointB,
				series: [
					{ minute: fixtureCatalog.timestamp.t20260308T000000(), count: 2 },
					{ minute: fixtureCatalog.timestamp.t20260308T000100(), count: 3 },
					{ minute: fixtureCatalog.timestamp.t20260308T000200(), count: 3 },
					{ minute: fixtureCatalog.timestamp.t20260308T003000(), count: 1 },
					{ minute: fixtureCatalog.timestamp.t20260308T005500(), count: 1 },
					{ minute: fixtureCatalog.timestamp.t20260308T005800(), count: 1 },
					{ minute: fixtureCatalog.timestamp.t20260308T005900(), count: 2 },
				],
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
