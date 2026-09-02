import type {
	AdminServiceMonitor,
	ServiceMonitorHistoryResponse,
	ServiceMonitorStatusResponse,
	ServiceMonitorSummary,
} from "../../src/api/adminServiceMonitors";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

const NOW = Math.floor(Date.now() / 1000);

function recentSummary(
	availability: number | null,
	coverage: number | null,
	expected: number,
	executed: number,
	slots: ServiceMonitorSummary["recent_6h"]["slots"],
	latestLatency: number | null = 42,
): ServiceMonitorSummary["recent_6h"] {
	return {
		availability_percent: availability,
		coverage_percent: coverage,
		expected,
		executed,
		latest_latency_ms: latestLatency,
		latest_observed_at_unix_seconds: NOW - 18,
		slots,
	};
}

function statusSlots(
	status: ServiceMonitorSummary["status"],
	overrides: Record<number, ServiceMonitorSummary["status"]> = {},
): ServiceMonitorSummary["recent_6h"]["slots"] {
	return Array.from({ length: 72 }, (_, index) => overrides[index] ?? status);
}

const MONITORS: ServiceMonitorSummary[] = [
	{
		monitor_id: "01JMONITOR00000000000000000",
		name: "Public landing page",
		target: {
			kind: "http",
			url: "http://www.example.net/",
			method: "head",
			accepted_statuses: [{ start: 200, end: 399 }],
		},
		interval_seconds: 300,
		observer_policy: { mode: "exclude", node_ids: [] },
		lifecycle: "active",
		revision: 1,
		revision_effective_at_unix_seconds: NOW - 300,
		status: "up",
		stale: false,
		quality: "complete",
		recent_6h: recentSummary(100, 100, 72, 72, statusSlots("up"), 63),
	},
	{
		monitor_id: "01JMONITOR00000000000000001",
		name: "Public API health",
		target: {
			kind: "https",
			url: "https://status.example.net/health",
			method: "get",
			accepted_statuses: [{ start: 200, end: 399 }],
		},
		interval_seconds: 60,
		observer_policy: { mode: "exclude", node_ids: [] },
		lifecycle: "active",
		revision: 3,
		revision_effective_at_unix_seconds: NOW - 60,
		status: "up",
		stale: false,
		quality: "complete",
		recent_6h: recentSummary(
			99.72,
			100,
			360,
			360,
			statusSlots("up", { 33: "degraded" }),
			42,
		),
	},
	{
		monitor_id: "01JMONITOR00000000000000002",
		name: "TCP edge port",
		target: {
			kind: "tcping",
			host: fixtureCatalog.host.serverPrimary(),
			port: fixtureCatalog.endpoint.port443(),
		},
		interval_seconds: 300,
		observer_policy: {
			mode: "include",
			node_ids: ["01JNODE0000000000000000001"],
		},
		lifecycle: "active",
		revision: 1,
		revision_effective_at_unix_seconds: NOW - 300,
		status: "degraded",
		stale: false,
		quality: "partial",
		recent_6h: recentSummary(
			97.22,
			97.22,
			72,
			70,
			statusSlots("up", { 36: "down", 37: "unknown", 38: "degraded" }),
			218,
		),
	},
	{
		monitor_id: "01JMONITOR00000000000000003",
		name: "DNS reachability",
		target: { kind: "ping", host: fixtureCatalog.host.primary() },
		interval_seconds: 900,
		observer_policy: {
			mode: "include",
			node_ids: ["01JNODE0000000000000000001", "01JNODE0000000000000000002"],
		},
		lifecycle: "active",
		revision: 2,
		revision_effective_at_unix_seconds: NOW - 900,
		status: "capture_suspended",
		stale: false,
		quality: "local_only",
		recent_6h: recentSummary(
			null,
			0,
			24,
			0,
			statusSlots("capture_suspended"),
			null,
		),
	},
];

export function defaultServiceMonitors(): ServiceMonitorSummary[] {
	return structuredClone(MONITORS);
}

export function monitorDefinition(
	monitor: ServiceMonitorSummary,
): AdminServiceMonitor {
	const {
		status: _status,
		stale: _stale,
		quality: _quality,
		recent_6h: _recent6h,
		...definition
	} = monitor;
	return structuredClone(definition);
}

export function monitorStatus(
	monitor: ServiceMonitorSummary,
): ServiceMonitorStatusResponse {
	return {
		monitor_id: monitor.monitor_id,
		status: monitor.status,
		stale: monitor.stale,
		freshness_seconds: 18,
		capture: {
			suspended: monitor.status === "capture_suspended",
			pending_observations: 3,
			pending_bytes: 7_680,
		},
		quality: monitor.quality,
		observers: [
			{
				node_id: fixtureCatalog.identifier.nodePrimary(),
				state: monitor.status,
				latest: {
					monitor_id: monitor.monitor_id,
					revision: monitor.revision,
					observer_node_id: fixtureCatalog.identifier.nodePrimary(),
					slot_unix_seconds: NOW - 60,
					observed_at_unix_seconds: NOW - 18,
					outcome:
						monitor.status === "up"
							? "success"
							: monitor.status === "capture_suspended"
								? "suspended"
								: "failure",
					error: monitor.status === "up" ? null : "connect_timeout",
					latency_ms:
						monitor.status === "up"
							? fixtureCatalog.number.value42()
							: fixtureCatalog.metric.none(),
					status_code: monitor.status === "up" ? 200 : null,
					packet_loss_percent: 0,
					ad_hoc: false,
				},
				icmp_supported: true,
			},
			{
				node_id: fixtureCatalog.identifier.nodeSecondary(),
				state: monitor.status === "up" ? "up" : "down",
				latest: null,
				icmp_supported: false,
			},
		],
	};
}

export function monitorHistory(
	monitor: ServiceMonitorSummary,
): ServiceMonitorHistoryResponse {
	const points = Array.from({ length: 24 }, (_, index) => {
		const failures = index === 13 && monitor.status !== "up" ? 1 : 0;
		const successes = 5 - failures;
		return {
			start_unix_seconds: NOW - (24 - index) * 3_600,
			end_unix_seconds: NOW - (23 - index) * 3_600 - 1,
			rollup: {
				expected: 5,
				executed: 5,
				successes,
				failures,
				unsupported: 0,
				suspended: 0,
				latency_count: successes,
				latency_sum_ms: successes * (36 + index),
				latency_min_ms: 29,
				latency_max_ms: 81,
				latency_histogram: {
					underflow: 0,
					buckets: Array.from({ length: 32 }, (_, bucket) =>
						bucket === 5 ? successes : 0,
					),
					overflow: 0,
				},
				errors: failures
					? { connect_timeout: failures }
					: ({} as Record<string, number>),
			},
			availability_percent: successes * 20,
			coverage_percent: 100,
		};
	});
	return {
		monitor_id: monitor.monitor_id,
		resolution: "1m",
		points,
		truncated: false,
		quality: monitor.quality,
		coverage_percent: 100,
		watermark_unix_seconds: NOW - 18,
		gaps: [],
		skew_seconds: 0,
		freshness_seconds: 18,
	};
}
