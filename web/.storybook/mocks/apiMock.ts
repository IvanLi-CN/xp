import type { AlertsResponse } from "../../src/api/adminAlerts";
import type {
	AdminEndpoint,
	AdminEndpointCreateRequest,
	AdminEndpointKind,
	AdminEndpointPatchRequest,
} from "../../src/api/adminEndpoints";
import type {
	AdminIpUsageWindow,
	AdminNodeIpUsageResponse,
	AdminUserIpUsageResponse,
} from "../../src/api/adminIpUsage";
import type { NodeHistorySnapshot } from "../../src/api/adminNodeHistory";
import type {
	AdminNodeRuntimeDetailResponse,
	AdminNodeRuntimeListItem,
	NodeRuntimeComponent,
	NodeRuntimeEvent,
	NodeRuntimeHistorySlot,
} from "../../src/api/adminNodeRuntime";
import type { AdminNode } from "../../src/api/adminNodes";
import type { AdminQuotaPolicyGlobalWeightRow } from "../../src/api/adminQuotaPolicyGlobalWeightRows";
import type { AdminQuotaPolicyNodePolicy } from "../../src/api/adminQuotaPolicyNodePolicy";
import type { AdminQuotaPolicyNodeWeightRow } from "../../src/api/adminQuotaPolicyNodeWeightRows";
import type { AdminRealityDomain } from "../../src/api/adminRealityDomains";
import type {
	AdminNodeTcpConnectionsResponse,
	AdminTcpConnectionUsageWindow,
} from "../../src/api/adminTcpConnections";
import type {
	AdminMihomoRedactRequest,
	AdminMihomoRedactResponse,
} from "../../src/api/adminTools";
import type {
	AdminNodeTrafficResponse,
	AdminUserTrafficResponse,
	TrafficReport,
	TrafficWindow,
} from "../../src/api/adminTraffic";
import type { AdminUpgradeStatusResponse } from "../../src/api/adminUpgrade";
import type { AdminUserAccessItem } from "../../src/api/adminUserAccess";
import type { AdminUserNodeQuotaStatusResponse } from "../../src/api/adminUserNodeQuotaStatus";
import type { AdminUserNodeWeightItem } from "../../src/api/adminUserNodeWeights";
import type { AdminUserQuotaSummariesResponse } from "../../src/api/adminUserQuotaSummaries";
import type {
	AdminUser,
	AdminUserCreateRequest,
	AdminUserPatchRequest,
	AdminUserTokenResponse,
} from "../../src/api/adminUsers";
import type { NodeQuotaReset, UserQuotaReset } from "../../src/api/quotaReset";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";
import {
	DEFAULT_API_CAPABILITIES,
	type MockStateSeed,
} from "./apiMockContract";
import { buildEndpointCreateMeta } from "./buildEndpointCreateMeta";
import { handleEndpointProbeRequest } from "./endpointProbeMock";
import {
	buildNodeDeletePreviewEndpoint,
	buildUserNodeQuotaStatusItem,
} from "./staticFixtureMappings";

export type StorybookApiMockConfig = {
	adminToken?: string | null;
	data?: Partial<MockStateSeed>;
	probe?: Parameters<typeof handleEndpointProbeRequest>[1];
	failAdminConfig?: boolean;
	failNodeRuntimeNodeIds?: string[];
	failVersionCheck?: boolean;
};

export type MockEndpointSeed = AdminEndpoint & {
	active_short_id?: string;
	short_ids?: string[];
};

type MockEndpointRecord = AdminEndpoint & {
	active_short_id: string;
	short_ids: string[];
};

export type MockWindowedNodeIpUsage =
	| AdminNodeIpUsageResponse
	| Partial<Record<AdminIpUsageWindow, AdminNodeIpUsageResponse>>;

export type MockWindowedNodeTcpConnections =
	| AdminNodeTcpConnectionsResponse
	| Partial<
			Record<AdminTcpConnectionUsageWindow, AdminNodeTcpConnectionsResponse>
	  >;

export type MockWindowedUserIpUsage =
	| AdminUserIpUsageResponse
	| Partial<Record<AdminIpUsageWindow, AdminUserIpUsageResponse>>;

export type MockWindowedNodeTraffic =
	| AdminNodeTrafficResponse
	| Partial<Record<TrafficWindow, AdminNodeTrafficResponse>>;

export type MockWindowedUserTraffic =
	| AdminUserTrafficResponse
	| Partial<Record<TrafficWindow, AdminUserTrafficResponse>>;

type MockState = Omit<MockStateSeed, "endpoints"> & {
	endpoints: MockEndpointRecord[];
	failAdminConfig: boolean;
	failNodeRuntimeNodeIds: string[];
	failVersionCheck: boolean;
	counters: {
		endpoint: number;
		joinToken: number;
		realityDomain: number;
		shortId: number;
		user: number;
	};
};

type MockApi = {
	reset: (config?: StorybookApiMockConfig) => void;
	handle: (req: Request) => Promise<Response>;
};

let singletonMock: MockApi | null = null;
let lastStoryKey = "";
let fetchInstalled = false;
let originalFetch: typeof fetch | null = null;

const JSON_HEADERS = { "Content-Type": "application/json" } as const;
const TEXT_HEADERS = { "Content-Type": "text/plain" } as const;
const DEFAULT_GLOBAL_WEIGHT = 100;

function selectWindowedNodeIpUsage(
	entry: MockWindowedNodeIpUsage,
	window: AdminIpUsageWindow,
): AdminNodeIpUsageResponse {
	if ("window_start" in entry) {
		return {
			...clone(entry),
			window,
		};
	}
	const report = entry[window] ?? entry["24h"] ?? entry["7d"];
	if (!report) {
		throw new Error(`missing node IP usage report for window ${window}`);
	}
	return {
		...clone(report),
		window,
	};
}

function selectWindowedNodeTcpConnections(
	entry: MockWindowedNodeTcpConnections,
	window: AdminTcpConnectionUsageWindow,
): AdminNodeTcpConnectionsResponse {
	if ("window_start" in entry) {
		return {
			...clone(entry),
			window,
		};
	}
	const report = entry[window] ?? entry["24h"] ?? entry["7d"];
	if (!report) {
		throw new Error(`missing node TCP connections report for window ${window}`);
	}
	return {
		...clone(report),
		window,
	};
}

function selectWindowedUserIpUsage(
	entry: MockWindowedUserIpUsage,
	window: AdminIpUsageWindow,
): AdminUserIpUsageResponse {
	if ("partial" in entry) {
		return {
			...clone(entry),
			window,
		};
	}
	const report = entry[window] ?? entry["24h"] ?? entry["7d"];
	if (!report) {
		throw new Error(`missing user IP usage report for window ${window}`);
	}
	return {
		...clone(report),
		window,
	};
}

function selectWindowedNodeTraffic(
	entry: MockWindowedNodeTraffic,
	window: TrafficWindow,
): AdminNodeTrafficResponse {
	if ("traffic" in entry) {
		return { ...clone(entry), traffic: { ...clone(entry.traffic), window } };
	}
	const report = entry[window] ?? entry["24h"] ?? entry["31d"];
	if (!report)
		throw new Error(`missing node traffic report for window ${window}`);
	return { ...clone(report), traffic: { ...clone(report.traffic), window } };
}

function selectWindowedUserTraffic(
	entry: MockWindowedUserTraffic,
	window: TrafficWindow,
): AdminUserTrafficResponse {
	if ("traffic" in entry) {
		return { ...clone(entry), traffic: { ...clone(entry.traffic), window } };
	}
	const report = entry[window] ?? entry["24h"] ?? entry["31d"];
	if (!report)
		throw new Error(`missing user traffic report for window ${window}`);
	return { ...clone(report), traffic: { ...clone(report.traffic), window } };
}

function clone<T>(value: T): T {
	if (typeof structuredClone === "function") {
		return structuredClone(value);
	}
	return JSON.parse(JSON.stringify(value)) as T;
}

function jsonResponse(
	data: unknown,
	init?: { status?: number; headers?: Record<string, string> },
): Response {
	return new Response(JSON.stringify(data), {
		status: init?.status ?? 200,
		headers: { ...JSON_HEADERS, ...init?.headers },
	});
}

function textResponse(
	data: string,
	init?: { status?: number; headers?: Record<string, string> },
): Response {
	return new Response(data, {
		status: init?.status ?? 200,
		headers: { ...TEXT_HEADERS, ...init?.headers },
	});
}

function errorResponse(
	status: number,
	code: string,
	message: string,
	details: Record<string, unknown> = {},
): Response {
	return jsonResponse(
		{
			error: {
				code,
				message,
				details,
			},
		},
		{ status },
	);
}

function sseResponse(
	events: Array<{ event: string; data: unknown }>,
): Response {
	const body = events
		.map((item) => {
			return `event: ${item.event}\ndata: ${JSON.stringify(item.data)}\n\n`;
		})
		.join("");
	return new Response(body, {
		status: 200,
		headers: {
			"Content-Type": "text/event-stream",
			"Cache-Control": "no-cache",
		},
	});
}

function ensureEndpointRecord(
	seed: MockEndpointSeed,
	_counters: MockState["counters"],
): MockEndpointRecord {
	return {
		endpoint_id: seed.endpoint_id,
		node_id: seed.node_id,
		tag: seed.tag,
		kind: seed.kind,
		port: seed.port,
		meta: seed.meta,
		probe: seed.probe,
		short_ids: fixtureCatalog.endpoint.shortIds(),
		active_short_id: fixtureCatalog.endpoint.activeShortId(),
	};
}

function buildRuntimeSlots(total = 7 * 24 * 2): NodeRuntimeHistorySlot[] {
	const slots: NodeRuntimeHistorySlot[] = [];
	for (let i = total - 1; i >= 0; i -= 1) {
		let status: NodeRuntimeHistorySlot["status"] = "up";
		if (i % 37 === 0) status = "degraded";
		if (i % 121 === 0) status = "down";
		if (i % 79 === 0) status = "unknown";
		slots.push({
			slot_start: fixtureCatalog.slotString.s6(),
			status,
		});
	}
	return slots;
}

function buildRuntimeComponents(node: AdminNode): NodeRuntimeComponent[] {
	const downNode = node.node_id.endsWith("2");
	return [
		{
			component: "xp",
			status: "up",
			last_ok_at: fixtureCatalog.slotString.s7(),
			last_fail_at: fixtureCatalog.optional.none(),
			down_since: fixtureCatalog.optional.none(),
			consecutive_failures: 0,
			recoveries_observed: 0,
			restart_attempts: 0,
			last_restart_at: fixtureCatalog.optional.none(),
			last_restart_fail_at: fixtureCatalog.optional.none(),
		},
		{
			component: "xray",
			status: downNode ? "down" : "up",
			last_ok_at: fixtureCatalog.slotString.s8(),
			last_fail_at: fixtureCatalog.slotString.s9(),
			down_since: fixtureCatalog.slotString.s10(),
			consecutive_failures: downNode ? 2 : 0,
			recoveries_observed: 1,
			restart_attempts: downNode ? 1 : 0,
			last_restart_at: fixtureCatalog.slotString.s11(),
			last_restart_fail_at: fixtureCatalog.optional.none(),
		},
		{
			component: "cloudflared",
			status: downNode ? "down" : "disabled",
			last_ok_at: fixtureCatalog.slotString.s12(),
			last_fail_at: fixtureCatalog.slotString.s13(),
			down_since: fixtureCatalog.slotString.s14(),
			consecutive_failures: downNode ? 3 : 0,
			recoveries_observed: 0,
			restart_attempts: downNode ? 1 : 0,
			last_restart_at: fixtureCatalog.slotString.s13(),
			last_restart_fail_at: fixtureCatalog.slotString.s13(),
		},
	];
}

function buildRuntimeEvents(node: AdminNode): NodeRuntimeEvent[] {
	return [
		{
			event_id: `evt-${node.node_id}-1`,
			occurred_at: fixtureCatalog.slotString.s15(),
			component: "xray",
			kind: "status_changed",
			message: "xray status changed: up -> down",
			from_status: "up",
			to_status: "down",
		},
		{
			event_id: `evt-${node.node_id}-2`,
			occurred_at: fixtureCatalog.slotString.s16(),
			component: "cloudflared",
			kind: "restart_failed",
			message: "cloudflared restart request failed",
			from_status: null,
			to_status: "down",
		},
	];
}

function buildNodeRuntimeListItem(node: AdminNode): AdminNodeRuntimeListItem {
	const components = buildRuntimeComponents(node);
	const slots = buildRuntimeSlots();
	const summaryStatus: AdminNodeRuntimeListItem["summary"]["status"] =
		components.some((component) => component.status === "down")
			? "degraded"
			: "up";
	return {
		node_id: node.node_id,
		node_name: node.node_name,
		api_base_url: node.api_base_url,
		access_host: node.access_host,
		summary: {
			status: summaryStatus,
			updated_at: fixtureCatalog.slotString.s7(),
		},
		components,
		recent_slots: slots,
	};
}

function buildNodeRuntimeDetail(
	node: AdminNode,
): AdminNodeRuntimeDetailResponse {
	const item = buildNodeRuntimeListItem(node);
	return {
		node: node,
		summary: item.summary,
		components: item.components,
		recent_slots: item.recent_slots,
		events: buildRuntimeEvents(node),
	};
}

function buildNodeHistory(node: AdminNode): NodeHistorySnapshot {
	const components = buildRuntimeComponents(node);
	return {
		node_id: node.node_id,
		last_synced_at: fixtureCatalog.slotString.s21(),
		last_sync_error: node.node_id.endsWith("2")
			? "request timeout while syncing node history"
			: null,
		daily_traffic: [
			{
				date: fixtureCatalog.timestamp.date(),
				uplink_bytes: fixtureCatalog.slotNumber.n0(),
				downlink_bytes: fixtureCatalog.slotNumber.n1(),
				updated_at: fixtureCatalog.slotString.s21(),
			},
		],
		daily_component_status: [
			{
				date: fixtureCatalog.timestamp.date(),
				components: components.map((component) => ({
					component: component.component,
					status: component.status,
					observed_at: fixtureCatalog.slotString.s21(),
				})),
			},
		],
		component_status_events: buildRuntimeEvents(node).map((event) => ({
			event_id: event.event_id,
			occurred_at: fixtureCatalog.slotString.s22(),
			component: event.component,
			message: event.message,
			from_status: event.from_status,
			to_status: event.to_status,
		})),
	};
}

function buildTrafficPoint(
	isCurrentDay: boolean,
	isGap = false,
): NonNullable<TrafficReport["current"]>[number] {
	if (isGap) {
		return {
			start_at: fixtureCatalog.timestamp.baseline(),
			end_at: fixtureCatalog.timestamp.recent(),
			uplink_bytes: null,
			downlink_bytes: null,
			total_bytes: null,
			complete: false,
			is_current_day: isCurrentDay,
		};
	}

	return {
		start_at: fixtureCatalog.timestamp.baseline(),
		end_at: fixtureCatalog.timestamp.recent(),
		uplink_bytes: fixtureCatalog.slotNumber.n2(),
		downlink_bytes: fixtureCatalog.slotNumber.n3(),
		total_bytes: fixtureCatalog.slotNumber.n6(),
		complete: true,
		is_current_day: isCurrentDay,
	};
}

function buildReferenceTrafficPoint(): NonNullable<
	TrafficReport["reference"]
>[number] {
	return {
		start_at: fixtureCatalog.timestamp.earlier(),
		end_at: fixtureCatalog.timestamp.baseline(),
		uplink_bytes: fixtureCatalog.slotNumber.n2(),
		downlink_bytes: fixtureCatalog.slotNumber.n3(),
		total_bytes: fixtureCatalog.slotNumber.n6(),
		complete: true,
		is_current_day: false,
	};
}

function buildTrafficReport(window: TrafficWindow, gap = false): TrafficReport {
	const count = window === "24h" ? 288 : 31;
	const current = Array.from({ length: count }, (_, index) =>
		buildTrafficPoint(
			window === "31d" && index === count - 1,
			gap && index > 112 && index < 124,
		),
	);
	const reference = Array.from({ length: count }, () =>
		buildReferenceTrafficPoint(),
	);
	const summary =
		window === "24h"
			? {
					uplink_bytes: fixtureCatalog.slotNumber.n32(),
					downlink_bytes: fixtureCatalog.slotNumber.n33(),
					total_bytes: fixtureCatalog.slotNumber.n34(),
				}
			: {
					uplink_bytes: fixtureCatalog.slotNumber.n35(),
					downlink_bytes: fixtureCatalog.slotNumber.n36(),
					total_bytes: fixtureCatalog.slotNumber.n37(),
				};
	return {
		window,
		window_start_at: fixtureCatalog.timestamp.baseline(),
		window_end_at: fixtureCatalog.timestamp.recent(),
		timezone: "UTC",
		summary: {
			mode: "cycle",
			cycle_start_at: fixtureCatalog.timestamp.earlier(),
			cycle_end_at: fixtureCatalog.timestamp.later(),
			...summary,
			complete: !gap,
			tracking_since: fixtureCatalog.timestamp.baseline(),
		},
		current,
		reference,
		partial: false,
		last_sample_at: fixtureCatalog.slotString.s23(),
		warnings: [],
	};
}

function buildDefaultNodeTraffic(node: AdminNode): MockWindowedNodeTraffic {
	return {
		"24h": { node, traffic: buildTrafficReport("24h") },
		"31d": { node, traffic: buildTrafficReport("31d") },
	};
}

function buildDefaultUserTraffic(
	user: AdminUser,
	nodes: AdminNode[],
): MockWindowedUserTraffic {
	const nodeOptions = nodes.map((node, index) => {
		const option = {
			node_id: fixtureCatalog.slotString.s36(),
			node_name: fixtureCatalog.slotString.s37(),
		};
		if (node.node_id === fixtureCatalog.identifier.nodePrimary()) {
			option.node_id = fixtureCatalog.identifier.nodePrimary();
			option.node_name = fixtureCatalog.identifier.nodeNameSecondary();
		} else if (node.node_id === fixtureCatalog.identifier.nodeSecondary()) {
			option.node_id = fixtureCatalog.identifier.nodeSecondary();
			option.node_name = fixtureCatalog.identifier.nodeNamePrimary();
		} else if (index === 0) {
			option.node_id = fixtureCatalog.slotString.s32();
			option.node_name = fixtureCatalog.slotString.s33();
		}
		return option;
	});
	return {
		"24h": {
			user: { user_id: user.user_id, display_name: user.display_name },
			traffic: buildTrafficReport("24h"),
			nodes: nodeOptions,
			partial: false,
			unreachable_nodes: [],
		},
		"31d": {
			user: { user_id: user.user_id, display_name: user.display_name },
			traffic: buildTrafficReport("31d"),
			nodes: nodeOptions,
			partial: false,
			unreachable_nodes: [],
		},
	};
}

function refreshGlobalEndpointReality(state: MockState): void {
	for (const endpoint of state.endpoints) {
		if (endpoint.kind !== "vless_reality_vision_tcp") continue;
		const meta = endpoint.meta as Record<string, unknown>;
		const reality = meta.reality as
			| undefined
			| null
			| {
					dest?: string;
					server_names?: string[];
					server_names_source?: string;
					fingerprint?: string;
			  };
		if (!reality || typeof reality !== "object") continue;
		if (reality.server_names_source !== "global") continue;

		meta.reality = fixtureCatalog.endpoint.reality();
	}
}

function buildDefaultNodeIpUsage(node: AdminNode): AdminNodeIpUsageResponse {
	return {
		node,
		window: "24h",
		geo_source: "country_is",
		window_start: fixtureCatalog.slotString.s24(),
		window_end: fixtureCatalog.slotString.s25(),
		warnings: [],
		unique_ip_series: [
			{ minute: fixtureCatalog.slotString.s24(), count: 1 },
			{ minute: fixtureCatalog.slotString.s26(), count: 2 },
			{ minute: fixtureCatalog.slotString.s25(), count: 1 },
		],
		timeline: [
			{
				lane_key: fixtureCatalog.slotString.s29(),
				endpoint_id: fixtureCatalog.slotString.s27(),
				endpoint_tag: fixtureCatalog.slotString.s28(),
				ip: fixtureCatalog.slotString.s29(),
				minutes: 2,
				segments: [
					{
						start_minute: fixtureCatalog.slotString.s24(),
						end_minute: fixtureCatalog.slotString.s26(),
					},
				],
			},
		],
		ips: [
			{
				ip: fixtureCatalog.slotString.s29(),
				minutes: 2,
				endpoint_tags: [fixtureCatalog.slotString.s28()],
				region: "Japan / Tokyo",
				operator: "ExampleNet",
				last_seen_at: fixtureCatalog.slotString.s26(),
			},
		],
	};
}

function buildDefaultUserIpUsage(
	user: AdminUser,
	nodes: AdminNode[],
): AdminUserIpUsageResponse {
	const groups: AdminUserIpUsageResponse["groups"] = nodes
		.slice(0, 2)
		.map((node, index) => {
			const endpointTag = fixtureCatalog.slotString.s28();
			return {
				node,
				geo_source: index === 0 ? "country_is" : "country_is",
				window_start: fixtureCatalog.slotString.s24(),
				window_end: fixtureCatalog.slotString.s25(),
				warnings: [],
				unique_ip_series: [
					{ minute: fixtureCatalog.slotString.s24(), count: 1 },
					{ minute: fixtureCatalog.slotString.s26(), count: 1 },
				],
				timeline: [
					{
						lane_key: fixtureCatalog.slotString.s31(),
						endpoint_id: fixtureCatalog.slotString.s30(),
						endpoint_tag: fixtureCatalog.slotString.s28(),
						ip: fixtureCatalog.slotString.s31(),
						minutes: 2,
						segments: [
							{
								start_minute: fixtureCatalog.slotString.s24(),
								end_minute: fixtureCatalog.slotString.s26(),
							},
						],
					},
				],
				ips: [
					{
						ip: fixtureCatalog.slotString.s31(),
						minutes: 2,
						endpoint_tags: [endpointTag],
						region: index === 0 ? "Japan / Tokyo" : "Japan / Osaka",
						operator: index === 0 ? "ExampleNet" : "CarrierNet",
						last_seen_at: fixtureCatalog.slotString.s26(),
					},
				],
			};
		});

	return {
		user: {
			user_id: user.user_id,
			display_name: user.display_name,
		},
		window: "24h",
		partial: false,
		unreachable_nodes: [],
		warnings: [],
		groups,
	};
}

function createDefaultSeed(): MockStateSeed {
	const nodes: AdminNode[] = [
		{
			node_id: fixtureCatalog.slotString.s32(),
			node_name: fixtureCatalog.slotString.s33(),
			api_base_url: fixtureCatalog.slotString.s34(),
			access_host: fixtureCatalog.slotString.s35(),
			quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
			quota_reset: fixtureCatalog.quota.reset() as NodeQuotaReset,
		},
		{
			node_id: fixtureCatalog.slotString.s36(),
			node_name: fixtureCatalog.slotString.s37(),
			api_base_url: fixtureCatalog.slotString.s38(),
			access_host: fixtureCatalog.slotString.s39(),
			quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
			quota_reset: fixtureCatalog.quota.reset() as NodeQuotaReset,
		},
	];

	const endpoints: MockEndpointSeed[] = [
		{
			endpoint_id: fixtureCatalog.slotString.s40(),
			node_id: fixtureCatalog.slotString.s32(),
			tag: fixtureCatalog.slotString.s41(),
			kind: fixtureCatalog.endpoint.vlessKind(),
			port: fixtureCatalog.endpoint.port443(),
			meta: {
				reality: fixtureCatalog.endpoint.reality(),
				reality_keys: fixtureCatalog.endpoint.realityKeys(),
				short_ids: fixtureCatalog.endpoint.shortIds(),
				active_short_id: fixtureCatalog.endpoint.activeShortId(),
			},
			short_ids: fixtureCatalog.endpoint.shortIds(),
			active_short_id: fixtureCatalog.endpoint.activeShortId(),
		},
		{
			endpoint_id: fixtureCatalog.slotString.s43(),
			node_id: fixtureCatalog.slotString.s36(),
			tag: fixtureCatalog.slotString.s44(),
			kind: fixtureCatalog.endpoint.ssKind(),
			port: fixtureCatalog.endpoint.port8443(),
			meta: {
				server_psk_b64: fixtureCatalog.endpoint.serverPskB64(),
			},
			short_ids: fixtureCatalog.endpoint.shortIds(),
			active_short_id: fixtureCatalog.endpoint.activeShortId(),
		},
	];

	const realityDomains: AdminRealityDomain[] = [
		{
			domain_id: fixtureCatalog.identifier.endpointPrimary(),
			server_name: fixtureCatalog.host.serverPrimary(),
			disabled_node_ids: [],
		},
		{
			domain_id: fixtureCatalog.identifier.endpointSecondary(),
			server_name: fixtureCatalog.host.serverSecondary(),
			disabled_node_ids: [],
		},
		{
			domain_id: fixtureCatalog.identifier.endpointTertiary(),
			server_name: fixtureCatalog.host.tertiary(),
			disabled_node_ids: [fixtureCatalog.slotString.s36()],
		},
	];

	const userId1 = fixtureCatalog.identifier.userPrimary();
	const userId2 = fixtureCatalog.identifier.userSecondary();
	const subToken1 = fixtureCatalog.slotString.s45();
	const subToken2 = fixtureCatalog.slotString.s46();

	const users: AdminUser[] = [
		{
			user_id: userId1,
			display_name: "Alice",
			subscription_token: fixtureCatalog.slotString.s45(),
			credential_epoch: fixtureCatalog.user.credentialEpoch(),
			priority_tier: fixtureCatalog.user.priorityTierDefault(),
			quota_reset: fixtureCatalog.quota.reset() as UserQuotaReset,
		},
		{
			user_id: userId2,
			display_name: "Bob",
			subscription_token: fixtureCatalog.slotString.s46(),
			credential_epoch: fixtureCatalog.user.credentialEpoch(),
			priority_tier: fixtureCatalog.user.priorityTierDefault(),
			quota_reset: fixtureCatalog.quota.reset() as UserQuotaReset,
		},
	];

	const userNodeWeights: Record<string, AdminUserNodeWeightItem[]> = {
		[userId1]: [
			{
				node_id: fixtureCatalog.slotString.s32(),
				weight: fixtureCatalog.slotNumber.n30(),
			},
		],
		[userId2]: [],
	};
	const userGlobalWeights: Record<string, number> = {
		[userId1]: fixtureCatalog.slotNumber.n30(),
		[userId2]: fixtureCatalog.slotNumber.n30(),
	};
	const nodeWeightPolicies: Record<string, AdminQuotaPolicyNodePolicy> = {
		[fixtureCatalog.slotString.s32()]: {
			node_id: fixtureCatalog.slotString.s32(),
			inherit_global: true,
		},
		[fixtureCatalog.slotString.s36()]: {
			node_id: fixtureCatalog.slotString.s36(),
			inherit_global: true,
		},
	};
	const userAccessByUserId: Record<string, AdminUserAccessItem[]> = {
		[userId1]: [
			{
				user_id: userId1,
				endpoint_id: fixtureCatalog.slotString.s40(),
				node_id: fixtureCatalog.slotString.s32(),
			},
		],
		[userId2]: [
			{
				user_id: userId2,
				endpoint_id: fixtureCatalog.slotString.s43(),
				node_id: fixtureCatalog.slotString.s36(),
			},
		],
	};
	const userAutoAssignEndpointKindsByUserId = Object.fromEntries(
		Object.entries(userAccessByUserId).map(([userId, items]) => [
			userId,
			inferAutoAssignEndpointKindsFromEndpoints(endpoints, items),
		]),
	);

	const alerts: AlertsResponse = {
		partial: false,
		unreachable_nodes: [],
		items: [],
	};

	const subscriptions: Record<string, string> = {
		[subToken1]: fixtureCatalog.subscription.rawUri(),
		[subToken2]: fixtureCatalog.subscription.rawUri(),
	};
	const nodeIpUsageByNodeId = Object.fromEntries(
		nodes.map((node) => [node.node_id, buildDefaultNodeIpUsage(node)]),
	) satisfies Record<string, AdminNodeIpUsageResponse>;
	const nodeTcpConnectionsByNodeId = Object.fromEntries(
		nodes.map((node) => [
			node.node_id,
			{
				node,
				window: "24h" as const,
				window_start: fixtureCatalog.slotString.s47(),
				window_end: fixtureCatalog.slotString.s48(),
				warnings: [],
				endpoints: [
					{
						endpoint_id: fixtureCatalog.slotString.s27(),
						endpoint_tag: fixtureCatalog.slotString.s49(),
						port: fixtureCatalog.endpoint.port443(),
					},
					{
						endpoint_id: fixtureCatalog.slotString.s50(),
						endpoint_tag: fixtureCatalog.slotString.s51(),
						port: fixtureCatalog.endpoint.port8443(),
					},
				],
				per_endpoint_series: [
					{
						endpoint_id: fixtureCatalog.slotString.s27(),
						endpoint_tag: fixtureCatalog.slotString.s49(),
						port: fixtureCatalog.endpoint.port443(),
						series: [
							{ minute: fixtureCatalog.slotString.s52(), count: 2 },
							{ minute: fixtureCatalog.slotString.s48(), count: 3 },
						],
					},
					{
						endpoint_id: fixtureCatalog.slotString.s50(),
						endpoint_tag: fixtureCatalog.slotString.s51(),
						port: fixtureCatalog.endpoint.port8443(),
						series: [
							{ minute: fixtureCatalog.slotString.s52(), count: 1 },
							{ minute: fixtureCatalog.slotString.s48(), count: 2 },
						],
					},
				],
			},
		]),
	) satisfies Record<string, AdminNodeTcpConnectionsResponse>;
	const nodeHistoryByNodeId = Object.fromEntries(
		nodes.map((node) => [node.node_id, buildNodeHistory(node)]),
	) satisfies Record<string, NodeHistorySnapshot>;
	const userIpUsageByUserId = Object.fromEntries(
		users.map((user) => [user.user_id, buildDefaultUserIpUsage(user, nodes)]),
	) satisfies Record<string, AdminUserIpUsageResponse>;
	const nodeTrafficByNodeId = Object.fromEntries(
		nodes.map((node) => [node.node_id, buildDefaultNodeTraffic(node)]),
	) satisfies Record<string, MockWindowedNodeTraffic>;
	const userTrafficByUserId = Object.fromEntries(
		users.map((user) => [user.user_id, buildDefaultUserTraffic(user, nodes)]),
	) satisfies Record<string, MockWindowedUserTraffic>;

	return {
		health: { status: "ok" },
		clusterInfo: {
			cluster_id: fixtureCatalog.slotString.s53(),
			node_id: fixtureCatalog.slotString.s32(),
			role: "leader",
			leader_api_base_url: fixtureCatalog.slotString.s34(),
			term: 12,
			xp_version: "0.0.0",
		},
		versionCheck: {
			current: { package: "0.0.0", release_tag: "v0.0.0" },
			latest: {
				release_tag: "v0.0.0",
				published_at: fixtureCatalog.slotString.s54(),
			},
			has_update: false,
			checked_at: fixtureCatalog.slotString.s54(),
			compare_reason: "semver",
			source: {
				kind: "github-releases",
				repo: "IvanLi-CN/xp",
				api_base: "https://api.github.com",
				channel: "stable",
			},
		},
		capabilities: DEFAULT_API_CAPABILITIES,
		nodes,
		endpoints,
		realityDomains,
		users,
		userAccessByUserId,
		userAutoAssignEndpointKindsByUserId,
		nodeQuotas: [],
		nodeIpUsageByNodeId,
		nodeTcpConnectionsByNodeId,
		nodeHistoryByNodeId,
		userIpUsageByUserId,
		nodeTrafficByNodeId,
		userTrafficByUserId,
		userNodeWeights,
		userGlobalWeights,
		nodeWeightPolicies,
		alerts,
		subscriptions,
	};
}

function buildState(config?: StorybookApiMockConfig): MockState {
	const base = createDefaultSeed();
	const overrides = config?.data;

	const merged: MockStateSeed = {
		health: overrides?.health ?? base.health,
		clusterInfo: overrides?.clusterInfo ?? base.clusterInfo,
		versionCheck: overrides?.versionCheck ?? base.versionCheck,
		capabilities: overrides?.capabilities ?? base.capabilities,
		nodes: overrides?.nodes ?? base.nodes,
		endpoints: overrides?.endpoints ?? base.endpoints,
		realityDomains: overrides?.realityDomains ?? base.realityDomains,
		users: overrides?.users ?? base.users,
		userAccessByUserId: {
			...base.userAccessByUserId,
			...(overrides?.userAccessByUserId ?? {}),
		},
		userAutoAssignEndpointKindsByUserId: {
			...base.userAutoAssignEndpointKindsByUserId,
			...(overrides?.userAutoAssignEndpointKindsByUserId ?? {}),
		},
		nodeQuotas: overrides?.nodeQuotas ?? base.nodeQuotas,
		nodeIpUsageByNodeId: {
			...base.nodeIpUsageByNodeId,
			...(overrides?.nodeIpUsageByNodeId ?? {}),
		},
		nodeTcpConnectionsByNodeId: {
			...base.nodeTcpConnectionsByNodeId,
			...(overrides?.nodeTcpConnectionsByNodeId ?? {}),
		},
		nodeHistoryByNodeId: {
			...base.nodeHistoryByNodeId,
			...(overrides?.nodeHistoryByNodeId ?? {}),
		},
		userIpUsageByUserId: {
			...base.userIpUsageByUserId,
			...(overrides?.userIpUsageByUserId ?? {}),
		},
		nodeTrafficByNodeId: {
			...base.nodeTrafficByNodeId,
			...(overrides?.nodeTrafficByNodeId ?? {}),
		},
		userTrafficByUserId: {
			...base.userTrafficByUserId,
			...(overrides?.userTrafficByUserId ?? {}),
		},
		userNodeWeights: overrides?.userNodeWeights ?? base.userNodeWeights,
		userGlobalWeights: overrides?.userGlobalWeights ?? base.userGlobalWeights,
		nodeWeightPolicies:
			overrides?.nodeWeightPolicies ?? base.nodeWeightPolicies,
		quotaSummaries: overrides?.quotaSummaries ?? base.quotaSummaries,
		alerts: overrides?.alerts ?? base.alerts,
		subscriptions: {
			...base.subscriptions,
			...(overrides?.subscriptions ?? {}),
		},
	};

	const counters = {
		endpoint: 1,
		joinToken: 1,
		realityDomain: 1,
		shortId: 1,
		subscription: 1,
		user: 1,
	};

	const endpoints = merged.endpoints.map((endpoint) =>
		ensureEndpointRecord(endpoint, counters),
	);

	const state: MockState = {
		...clone(merged),
		endpoints,
		failAdminConfig: config?.failAdminConfig ?? false,
		failNodeRuntimeNodeIds: config?.failNodeRuntimeNodeIds ?? [],
		failVersionCheck: config?.failVersionCheck ?? false,
		counters,
	};

	refreshGlobalEndpointReality(state);
	return state;
}

function buildSubscriptionText(format: string | null): string {
	if (format === "clash") {
		return fixtureCatalog.subscription.clash();
	}
	if (format === "mihomo_provider") {
		return fixtureCatalog.subscription.rawUri();
	}
	if (format === "mihomo_provider_system") {
		return fixtureCatalog.subscription.clash();
	}
	if (format === "mihomo") {
		return fixtureCatalog.subscription.clash();
	}
	return fixtureCatalog.subscription.rawUri();
}

async function readJson<T>(req: Request): Promise<T | undefined> {
	const text = await req.text();
	if (!text) return undefined;
	try {
		return JSON.parse(text) as T;
	} catch {
		return undefined;
	}
}

function ensureUserAccessStore(
	state: MockState,
	userId: string,
): AdminUserAccessItem[] {
	const existing = state.userAccessByUserId[userId];
	if (existing) return existing;
	const items: AdminUserAccessItem[] = [];
	state.userAccessByUserId[userId] = items;
	return items;
}

function inferAutoAssignEndpointKindsFromEndpoints(
	endpoints: Array<Pick<AdminEndpoint, "endpoint_id" | "kind">>,
	items: AdminUserAccessItem[],
): AdminEndpointKind[] {
	const selectedEndpointIds = new Set(items.map((item) => item.endpoint_id));
	const endpointIdsByKind = new Map<AdminEndpointKind, Set<string>>();
	for (const endpoint of endpoints) {
		const endpointIds =
			endpointIdsByKind.get(endpoint.kind) ?? new Set<string>();
		endpointIds.add(endpoint.endpoint_id);
		endpointIdsByKind.set(endpoint.kind, endpointIds);
	}

	const out: AdminEndpointKind[] = [];
	for (const [kind, endpointIds] of endpointIdsByKind) {
		if (
			[...endpointIds].every((endpointId) =>
				selectedEndpointIds.has(endpointId),
			)
		) {
			out.push(kind);
		}
	}
	return out.sort();
}

function inferAutoAssignEndpointKinds(
	state: MockState,
	items: AdminUserAccessItem[],
): AdminEndpointKind[] {
	return inferAutoAssignEndpointKindsFromEndpoints(state.endpoints, items);
}

function autoAssignKindsForUser(
	state: MockState,
	userId: string,
): AdminEndpointKind[] {
	return state.userAutoAssignEndpointKindsByUserId[userId] ?? [];
}

function setAutoAssignKindsForUser(
	state: MockState,
	userId: string,
	kinds: AdminEndpointKind[],
) {
	if (kinds.length === 0) {
		delete state.userAutoAssignEndpointKindsByUserId[userId];
		return;
	}
	state.userAutoAssignEndpointKindsByUserId[userId] = kinds;
}

function applyAutoAssignForEndpoint(
	state: MockState,
	endpoint: MockEndpointRecord,
) {
	for (const [userId, kinds] of Object.entries(
		state.userAutoAssignEndpointKindsByUserId,
	)) {
		if (!kinds.includes(endpoint.kind)) continue;
		const items = ensureUserAccessStore(state, userId);
		if (items.some((item) => item.endpoint_id === endpoint.endpoint_id)) {
			continue;
		}
		items.push({
			user_id: userId,
			endpoint_id: fixtureCatalog.identifier.endpointSecondary(),
			node_id: fixtureCatalog.slotString.s32(),
		});
		items.sort((a, b) => a.endpoint_id.localeCompare(b.endpoint_id));
	}
}

function mockRedactOutput(payload: AdminMihomoRedactRequest): string {
	let source = payload.source;
	if (payload.source_kind === "url") {
		source = `url: ${payload.source}`;
	}
	return source
		.replaceAll("edge.example.com", "e***.example.com")
		.replaceAll("super-secret", "supe***cret")
		.replaceAll(
			"12345678-1234-1234-1234-123456789abc",
			"1234************************9abc",
		)
		.replaceAll("public_key_value", "publ****alue");
}

function buildAdminUpgradeStatus(
	overrides?: Partial<AdminUpgradeStatusResponse>,
): AdminUpgradeStatusResponse {
	return {
		support: {
			supported: true,
			reason: null,
			trigger: "xp-upgrade-trigger",
			...overrides?.support,
		},
		status: {
			state: "idle",
			target_tag: null,
			repo: "IvanLi-CN/xp",
			started_at: null,
			finished_at: null,
			exit_code: null,
			message: null,
			updated_at: fixtureCatalog.slotString.s7(),
			...overrides?.status,
		},
	};
}

async function handleRequest(
	state: MockState,
	req: Request,
): Promise<Response> {
	const method = req.method.toUpperCase();
	const url = new URL(req.url, "http://localhost");
	const path = url.pathname;

	if (!path.startsWith("/api/")) {
		return errorResponse(404, "not_found", "mock only handles /api/* requests");
	}

	if (path === "/api/health" && method === "GET") {
		return jsonResponse(state.health);
	}

	if (path === "/api/cluster/info" && method === "GET") {
		return jsonResponse(state.clusterInfo);
	}

	if (path === "/api/capabilities" && method === "GET") {
		return jsonResponse(clone(state.capabilities));
	}

	if (path === "/api/version/check" && method === "GET") {
		if (state.failVersionCheck) {
			return errorResponse(502, "upstream_error", "mock version check failure");
		}
		return jsonResponse(clone(state.versionCheck));
	}

	if (path === "/api/admin/upgrade/status" && method === "GET") {
		return jsonResponse(buildAdminUpgradeStatus());
	}

	if (path === "/api/admin/upgrade/start" && method === "POST") {
		const payload = await readJson<{ target_tag?: string }>(req);
		return jsonResponse(
			buildAdminUpgradeStatus({
				status: {
					state: "running",
					target_tag: payload?.target_tag ?? "v0.0.0",
					started_at: fixtureCatalog.timestamp.recent(),
					updated_at: fixtureCatalog.slotString.s7(),
					message: "storybook mock upgrade started",
				},
			}),
		);
	}

	if (path === "/api/admin/status/events" && method === "GET") {
		const upgrade = buildAdminUpgradeStatus();
		const nodesRuntime = {
			partial: false,
			unreachable_nodes: [],
			items: state.nodes.map((node) => buildNodeRuntimeListItem(node)),
		};
		return sseResponse([
			{
				event: "hello",
				data: {
					node_id: fixtureCatalog.slotString.s57(),
					connected_at: fixtureCatalog.slotString.s7(),
				},
			},
			{
				event: "snapshot",
				data: {
					emitted_at: fixtureCatalog.timestamp.recent(),
					health: clone(state.health),
					cluster_info: clone(state.clusterInfo),
					nodes_runtime: clone(nodesRuntime),
					alerts: clone(state.alerts),
					upgrade,
				},
			},
		]);
	}

	if (path === "/api/admin/tools/mihomo/redact" && method === "POST") {
		const payload = await readJson<AdminMihomoRedactRequest>(req);
		if (!payload || payload.source.trim().length === 0) {
			return errorResponse(400, "invalid_request", "source is empty");
		}
		if (
			payload.source_kind === "url" &&
			/^https?:\/\/(localhost|127\.|10\.|192\.168\.|172\.(1[6-9]|2\d|3[01])\.)/i.test(
				payload.source,
			)
		) {
			return errorResponse(
				400,
				"invalid_request",
				"source url must resolve to public ip addresses",
			);
		}

		const response: AdminMihomoRedactResponse = {
			redacted_text: mockRedactOutput(payload),
		};
		return jsonResponse(response);
	}

	if (path === "/api/admin/config" && method === "GET") {
		if (state.failAdminConfig) {
			return errorResponse(500, "internal", "mock admin config failure");
		}
		return jsonResponse({
			bind: fixtureCatalog.slotString.s58(),
			xray_api_addr: fixtureCatalog.slotString.s59(),
			data_dir: "./data",
			node_name: fixtureCatalog.slotString.s60(),
			access_host: fixtureCatalog.slotString.s61(),
			api_base_url: fixtureCatalog.slotString.s62(),
			vless_https_canary_bind: fixtureCatalog.address.loopback39043(),
			quota_poll_interval_secs: 10,
			quota_auto_unban: true,
			ip_geo_enabled: false,
			ip_geo_origin: fixtureCatalog.url.publicOrigin(),
			mihomo_resource_allow_private_targets: false,
			admin_token_present: true,
			admin_token_masked: fixtureCatalog.subscription.providerPassword(),
		});
	}

	if (path === "/api/admin/mihomo/resource-policy" && method === "PUT") {
		const payload = await readJson<{ allow_private_targets?: boolean }>(req);
		return jsonResponse({
			allow_private_targets: payload?.allow_private_targets === true,
		});
	}

	if (path === "/api/admin/nodes" && method === "GET") {
		return jsonResponse({ items: clone(state.nodes) });
	}

	const nodeDeletePreviewMatch = path.match(
		/^\/api\/admin\/nodes\/([^/]+)\/delete-preview$/,
	);
	if (nodeDeletePreviewMatch && method === "GET") {
		const nodeId = decodeURIComponent(nodeDeletePreviewMatch[1]);
		const node = state.nodes.find((item) => item.node_id === nodeId);
		if (!node) {
			return errorResponse(404, "not_found", "node not found");
		}
		return jsonResponse({
			node_id: fixtureCatalog.identifier.nodePrimary(),
			endpoints: state.endpoints
				.filter((endpoint) => endpoint.node_id === nodeId)
				.map(buildNodeDeletePreviewEndpoint),
		});
	}

	const nodeEgressProbeRefreshMatch = path.match(
		/^\/api\/admin\/nodes\/([^/]+)\/egress-probe\/refresh$/,
	);
	if (nodeEgressProbeRefreshMatch && method === "POST") {
		const nodeId = decodeURIComponent(nodeEgressProbeRefreshMatch[1]);
		const node = state.nodes.find((item) => item.node_id === nodeId);
		if (!node) {
			return errorResponse(404, "not_found", "node not found");
		}
		return jsonResponse({
			node_id: fixtureCatalog.slotString.s32(),
			accepted: true,
			egress_probe: clone(node.egress_probe),
		});
	}

	if (path === "/api/admin/nodes/runtime" && method === "GET") {
		const items = state.nodes.map((node) => buildNodeRuntimeListItem(node));
		return jsonResponse({
			partial: false,
			unreachable_nodes: [],
			items: clone(items),
		});
	}

	const nodeRuntimeMatch = path.match(
		/^\/api\/admin\/nodes\/([^/]+)\/runtime$/,
	);
	if (nodeRuntimeMatch && method === "GET") {
		const nodeId = decodeURIComponent(nodeRuntimeMatch[1]);
		const node = state.nodes.find((item) => item.node_id === nodeId);
		if (!node) {
			return errorResponse(404, "not_found", "node not found");
		}
		if (state.failNodeRuntimeNodeIds.includes(nodeId)) {
			return errorResponse(
				504,
				"node_unreachable",
				"node runtime request timeout",
			);
		}
		return jsonResponse(clone(buildNodeRuntimeDetail(node)));
	}

	const nodeHistoryMatch = path.match(
		/^\/api\/admin\/nodes\/([^/]+)\/history$/,
	);
	if (nodeHistoryMatch && method === "GET") {
		const nodeId = decodeURIComponent(nodeHistoryMatch[1]);
		const node = state.nodes.find((item) => item.node_id === nodeId);
		if (!node) {
			return errorResponse(404, "not_found", "node not found");
		}
		return jsonResponse({
			node: clone(node),
			history: clone(state.nodeHistoryByNodeId[nodeId] ?? null),
		});
	}

	const nodeTrafficMatch = path.match(
		/^\/api\/admin\/nodes\/([^/]+)\/traffic$/,
	);
	if (nodeTrafficMatch && method === "GET") {
		const nodeId = decodeURIComponent(nodeTrafficMatch[1]);
		const node = state.nodes.find((item) => item.node_id === nodeId);
		const entry =
			state.nodeTrafficByNodeId[nodeId] ??
			(node ? buildDefaultNodeTraffic(node) : null);
		if (!entry) return errorResponse(404, "not_found", "node not found");
		const window = url.searchParams.get("window") === "31d" ? "31d" : "24h";
		return jsonResponse(selectWindowedNodeTraffic(entry, window));
	}

	const nodeRuntimeEventsMatch = path.match(
		/^\/api\/admin\/nodes\/([^/]+)\/runtime\/events$/,
	);
	if (nodeRuntimeEventsMatch && method === "GET") {
		const nodeId = decodeURIComponent(nodeRuntimeEventsMatch[1]);
		const node = state.nodes.find((item) => item.node_id === nodeId);
		if (!node) {
			return errorResponse(404, "not_found", "node not found");
		}
		const detail = buildNodeRuntimeDetail(node);
		return sseResponse([
			{
				event: "hello",
				data: {
					node_id: fixtureCatalog.slotString.s17(),
					connected_at: fixtureCatalog.slotString.s7(),
				},
			},
			{
				event: "snapshot",
				data: {
					node_id: fixtureCatalog.slotString.s17(),
					summary: detail.summary,
					components: detail.components,
					recent_slots: detail.recent_slots,
					events: detail.events,
				},
			},
		]);
	}

	const nodeIpUsageMatch = path.match(
		/^\/api\/admin\/nodes\/([^/]+)\/ip-usage$/,
	);
	if (nodeIpUsageMatch && method === "GET") {
		const nodeId = decodeURIComponent(nodeIpUsageMatch[1]);
		const node = state.nodes.find((item) => item.node_id === nodeId);
		const report =
			state.nodeIpUsageByNodeId[nodeId] ??
			(node ? buildDefaultNodeIpUsage(node) : null);
		if (!report) {
			return errorResponse(404, "not_found", "node not found");
		}
		const window = url.searchParams.get("window") === "7d" ? "7d" : "24h";
		return jsonResponse(selectWindowedNodeIpUsage(report, window));
	}

	const nodeTcpConnectionsMatch = path.match(
		/^\/api\/admin\/nodes\/([^/]+)\/tcp-connections$/,
	);
	if (nodeTcpConnectionsMatch && method === "GET") {
		const nodeId = decodeURIComponent(nodeTcpConnectionsMatch[1]);
		const node = state.nodes.find((item) => item.node_id === nodeId);
		const report =
			state.nodeTcpConnectionsByNodeId[nodeId] ?? (node ? null : null);
		if (!report) {
			return errorResponse(404, "not_found", "node not found");
		}
		const window = url.searchParams.get("window") === "7d" ? "7d" : "24h";
		return jsonResponse(selectWindowedNodeTcpConnections(report, window));
	}

	const userIpUsageMatch = path.match(
		/^\/api\/admin\/users\/([^/]+)\/ip-usage$/,
	);
	if (userIpUsageMatch && method === "GET") {
		const userId = decodeURIComponent(userIpUsageMatch[1]);
		const user = state.users.find((item) => item.user_id === userId);
		const report =
			state.userIpUsageByUserId[userId] ??
			(user ? buildDefaultUserIpUsage(user, state.nodes) : null);
		if (!report) {
			return errorResponse(404, "not_found", "user not found");
		}
		const window = url.searchParams.get("window") === "7d" ? "7d" : "24h";
		return jsonResponse(selectWindowedUserIpUsage(report, window));
	}

	const userTrafficMatch = path.match(
		/^\/api\/admin\/users\/([^/]+)\/traffic$/,
	);
	if (userTrafficMatch && method === "GET") {
		const userId = decodeURIComponent(userTrafficMatch[1]);
		const user = state.users.find((item) => item.user_id === userId);
		const entry =
			state.userTrafficByUserId[userId] ??
			(user ? buildDefaultUserTraffic(user, state.nodes) : null);
		if (!entry) return errorResponse(404, "not_found", "user not found");
		const window = url.searchParams.get("window") === "31d" ? "31d" : "24h";
		const selectedNodeId = url.searchParams.get("node_id");
		const report = selectWindowedUserTraffic(entry, window);
		if (selectedNodeId) {
			const node = report.nodes.find((item) => item.node_id === selectedNodeId);
			if (!node)
				return errorResponse(
					404,
					"not_found",
					"user has no traffic on the selected node",
				);
			return jsonResponse({ ...report, nodes: [node] });
		}
		return jsonResponse(report);
	}

	const userNodeQuotasMatch = path.match(
		/^\/api\/admin\/users\/([^/]+)\/node-quotas$/,
	);
	if (userNodeQuotasMatch && method === "GET") {
		const userId = decodeURIComponent(userNodeQuotasMatch[1]);
		const userExists = state.users.some((u) => u.user_id === userId);
		if (!userExists) {
			return errorResponse(404, "not_found", "user not found");
		}
		const items = state.nodeQuotas.filter((q) => q.user_id === userId);
		return jsonResponse({ items: clone(items) });
	}

	const userNodeQuotaPutMatch = path.match(
		/^\/api\/admin\/users\/([^/]+)\/node-quotas\/([^/]+)$/,
	);
	if (userNodeQuotaPutMatch && method === "PUT") {
		// Deprecated: static per-user node quotas are no longer editable.
		return errorResponse(
			410,
			"gone",
			"user node quotas are no longer editable; configure node quota_limit_bytes + user node weights instead",
		);
	}

	const userNodeWeightsMatch = path.match(
		/^\/api\/admin\/users\/([^/]+)\/node-weights$/,
	);
	if (userNodeWeightsMatch && method === "GET") {
		const userId = decodeURIComponent(userNodeWeightsMatch[1]);
		const userExists = state.users.some((u) => u.user_id === userId);
		if (!userExists) {
			return errorResponse(404, "not_found", "user not found");
		}
		const items = state.userNodeWeights[userId] ?? [];
		return jsonResponse({ items: clone(items) });
	}

	const userNodeWeightPutMatch = path.match(
		/^\/api\/admin\/users\/([^/]+)\/node-weights\/([^/]+)$/,
	);
	if (userNodeWeightPutMatch && method === "PUT") {
		const userId = decodeURIComponent(userNodeWeightPutMatch[1]);
		const nodeId = decodeURIComponent(userNodeWeightPutMatch[2]);
		const userExists = state.users.some((u) => u.user_id === userId);
		if (!userExists) {
			return errorResponse(404, "not_found", "user not found");
		}
		const nodeExists = state.nodes.some((n) => n.node_id === nodeId);
		if (!nodeExists) {
			return errorResponse(404, "not_found", "node not found");
		}

		const payload = await readJson<unknown>(req);
		if (!payload) {
			return errorResponse(400, "invalid_request", "invalid JSON payload");
		}

		const items = state.userNodeWeights[userId] ?? [];
		const next: AdminUserNodeWeightItem =
			nodeId === fixtureCatalog.slotString.s32()
				? {
						node_id: fixtureCatalog.slotString.s32(),
						weight: fixtureCatalog.slotNumber.n13(),
					}
				: {
						node_id: fixtureCatalog.slotString.s36(),
						weight: fixtureCatalog.slotNumber.n13(),
					};
		state.userNodeWeights[userId] = [
			...items.filter((i) => i.node_id !== nodeId),
			next,
		];

		return jsonResponse(clone(next));
	}

	const quotaPolicyNodeWeightRowsMatch = path.match(
		/^\/api\/admin\/quota-policy\/nodes\/([^/]+)\/weight-rows$/,
	);
	if (
		path === "/api/admin/quota-policy/global-weight-rows" &&
		method === "GET"
	) {
		const items: AdminQuotaPolicyGlobalWeightRow[] = state.users.map((user) => {
			const storedWeight = state.userGlobalWeights[user.user_id];
			return {
				user_id: user.user_id,
				display_name: user.display_name,
				priority_tier: user.priority_tier,
				stored_weight: storedWeight,
				editor_weight: storedWeight ?? DEFAULT_GLOBAL_WEIGHT,
				source: storedWeight === undefined ? "implicit_default" : "explicit",
			};
		});
		items.sort(
			(a, b) =>
				b.editor_weight - a.editor_weight || a.user_id.localeCompare(b.user_id),
		);
		return jsonResponse({ items: clone(items) });
	}

	const quotaPolicyGlobalWeightPutMatch = path.match(
		/^\/api\/admin\/quota-policy\/global-weight-rows\/([^/]+)$/,
	);
	if (quotaPolicyGlobalWeightPutMatch && method === "PUT") {
		const userId = decodeURIComponent(quotaPolicyGlobalWeightPutMatch[1]);
		const userExists = state.users.some((u) => u.user_id === userId);
		if (!userExists) {
			return errorResponse(404, "not_found", "user not found");
		}

		const payload = await readJson<unknown>(req);
		if (!payload) {
			return errorResponse(400, "invalid_request", "invalid JSON payload");
		}

		state.userGlobalWeights[userId] = fixtureCatalog.slotNumber.n13();
		return jsonResponse({
			user_id: userId,
			weight: fixtureCatalog.slotNumber.n13(),
		});
	}

	const quotaPolicyNodePolicyMatch = path.match(
		/^\/api\/admin\/quota-policy\/nodes\/([^/]+)\/policy$/,
	);
	if (quotaPolicyNodePolicyMatch) {
		const nodeId = decodeURIComponent(quotaPolicyNodePolicyMatch[1]);
		const nodeExists = state.nodes.some((node) => node.node_id === nodeId);
		if (!nodeExists) {
			return errorResponse(404, "not_found", "node not found");
		}
		if (method === "GET") {
			return jsonResponse(
				clone(
					state.nodeWeightPolicies[nodeId] ?? {
						node_id: fixtureCatalog.slotString.s32(),
						inherit_global: true,
					},
				),
			);
		}
		if (method === "PUT") {
			const payload = await readJson<{ inherit_global?: boolean }>(req);
			if (!payload || typeof payload.inherit_global !== "boolean") {
				return errorResponse(
					400,
					"invalid_request",
					"inherit_global must be a boolean",
				);
			}
			const nextPolicy: AdminQuotaPolicyNodePolicy = {
				node_id: fixtureCatalog.slotString.s32(),
				inherit_global: payload.inherit_global,
			};
			state.nodeWeightPolicies[nodeId] = nextPolicy;
			return jsonResponse(clone(nextPolicy));
		}
	}

	if (quotaPolicyNodeWeightRowsMatch && method === "GET") {
		const nodeId = decodeURIComponent(quotaPolicyNodeWeightRowsMatch[1]);
		const nodeExists = state.nodes.some((node) => node.node_id === nodeId);
		if (!nodeExists) {
			return errorResponse(404, "not_found", "node not found");
		}

		const endpointNodeById = new Map(
			state.endpoints.map((endpoint) => [
				endpoint.endpoint_id,
				endpoint.node_id,
			]),
		);
		const endpointIdsByUser = new Map<string, Set<string>>();
		for (const [userId, access] of Object.entries(state.userAccessByUserId)) {
			for (const membership of access) {
				const endpointNodeId = endpointNodeById.get(membership.endpoint_id);
				if (!endpointNodeId || endpointNodeId !== nodeId) {
					continue;
				}
				if (!endpointIdsByUser.has(userId)) {
					endpointIdsByUser.set(userId, new Set<string>());
				}
				endpointIdsByUser.get(userId)?.add(membership.endpoint_id);
			}
		}

		const items: AdminQuotaPolicyNodeWeightRow[] = [];
		for (const [userId, endpointIdsSet] of endpointIdsByUser.entries()) {
			const user = state.users.find(
				(candidate) => candidate.user_id === userId,
			);
			if (!user) {
				continue;
			}
			const storedWeight = (state.userNodeWeights[userId] ?? []).find(
				(entry) => entry.node_id === nodeId,
			)?.weight;
			items.push({
				user_id: user.user_id,
				display_name: user.display_name,
				priority_tier: user.priority_tier,
				endpoint_ids: [...endpointIdsSet].sort(),
				stored_weight: storedWeight,
				editor_weight: storedWeight ?? 0,
				source: storedWeight === undefined ? "implicit_zero" : "explicit",
			});
		}
		items.sort(
			(a, b) =>
				b.editor_weight - a.editor_weight || a.user_id.localeCompare(b.user_id),
		);
		return jsonResponse({ items: clone(items) });
	}

	const nodeMatch = path.match(/^\/api\/admin\/nodes\/([^/]+)$/);
	if (nodeMatch) {
		const nodeId = decodeURIComponent(nodeMatch[1]);
		const node = state.nodes.find((item) => item.node_id === nodeId);
		if (!node) {
			return errorResponse(404, "not_found", "node not found");
		}
		if (method === "GET") {
			return jsonResponse(clone(node));
		}
		if (method === "PATCH") {
			const payload = await readJson<{
				node_name?: string;
				access_host?: string;
				api_base_url?: string;
				quota_limit_bytes?: number;
				quota_reset?: NodeQuotaReset;
			}>(req);
			if (!payload) {
				return errorResponse(400, "invalid_request", "invalid JSON payload");
			}
			const updated: AdminNode = {
				...node,
				node_name: fixtureCatalog.slotString.s33(),
				access_host: fixtureCatalog.slotString.s35(),
				api_base_url: fixtureCatalog.slotString.s34(),
				quota_limit_bytes: fixtureCatalog.quota.limitBytes(),
				quota_reset: fixtureCatalog.quota.reset() as NodeQuotaReset,
			};
			state.nodes = state.nodes.map((item) =>
				item.node_id === nodeId ? updated : item,
			);
			return jsonResponse(clone(updated));
		}
		if (method === "DELETE") {
			const deleteEndpoints =
				url.searchParams.get("delete_endpoints") === "true";
			const endpoints = state.endpoints.filter(
				(endpoint) => endpoint.node_id === nodeId,
			);
			if (endpoints.length > 0 && !deleteEndpoints) {
				return errorResponse(
					409,
					"conflict",
					`node is still referenced by endpoints: node_id=${nodeId} endpoint_id=${endpoints[0].endpoint_id}`,
				);
			}
			const expectedEndpointIds = new Set(
				(url.searchParams.get("expected_endpoint_ids") ?? "")
					.split(",")
					.filter(Boolean),
			);
			if (
				endpoints.length > 0 &&
				(endpoints.length !== expectedEndpointIds.size ||
					endpoints.some(
						(endpoint) => !expectedEndpointIds.has(endpoint.endpoint_id),
					))
			) {
				return errorResponse(
					409,
					"conflict",
					`node endpoint set changed since delete preview: node_id=${nodeId}`,
				);
			}
			state.endpoints = state.endpoints.filter(
				(endpoint) => endpoint.node_id !== nodeId,
			);
			state.nodes = state.nodes.filter((item) => item.node_id !== nodeId);
			return new Response(null, { status: 204 });
		}
	}

	if (path === "/api/admin/cluster/join-tokens" && method === "POST") {
		const payload = await readJson<{ ttl_seconds?: number }>(req);
		if (!payload) {
			return errorResponse(400, "invalid_request", "invalid JSON payload");
		}
		return jsonResponse({
			join_token: fixtureCatalog.identifier.tokenTertiary(),
		});
	}

	if (path === "/api/admin/reality-domains" && method === "GET") {
		return jsonResponse({ items: clone(state.realityDomains) });
	}

	if (path === "/api/admin/reality-domains" && method === "POST") {
		const payload = await readJson<{
			server_name?: string;
			disabled_node_ids?: string[];
		}>(req);
		if (!payload || typeof payload.server_name !== "string") {
			return errorResponse(400, "invalid_request", "invalid JSON payload");
		}
		const domain: AdminRealityDomain = {
			domain_id: fixtureCatalog.identifier.endpointTertiary(),
			server_name: fixtureCatalog.host.tertiary(),
			disabled_node_ids: [],
		};
		state.realityDomains = [...state.realityDomains, domain];
		refreshGlobalEndpointReality(state);
		return jsonResponse(clone(domain));
	}

	if (path === "/api/admin/reality-domains/reorder" && method === "POST") {
		const payload = await readJson<{ domain_ids?: string[] }>(req);
		const ids = payload?.domain_ids;
		if (
			!payload ||
			!Array.isArray(ids) ||
			!ids.every((id) => typeof id === "string")
		) {
			return errorResponse(400, "invalid_request", "invalid JSON payload");
		}

		const byId = new Map(state.realityDomains.map((d) => [d.domain_id, d]));
		const next: AdminRealityDomain[] = [];
		for (const id of ids) {
			const domain = byId.get(id);
			if (!domain) {
				return errorResponse(
					400,
					"invalid_request",
					`unknown domain_id: ${id}`,
				);
			}
			next.push(domain);
		}
		state.realityDomains = next;
		refreshGlobalEndpointReality(state);
		return new Response(null, { status: 204 });
	}

	const realityDomainMatch = path.match(
		/^\/api\/admin\/reality-domains\/([^/]+)$/,
	);
	if (realityDomainMatch) {
		const domainId = decodeURIComponent(realityDomainMatch[1]);
		const existing = state.realityDomains.find((d) => d.domain_id === domainId);
		if (!existing) {
			return errorResponse(404, "not_found", "reality domain not found");
		}
		if (method === "PATCH") {
			const payload = await readJson<{
				server_name?: string;
				disabled_node_ids?: string[];
			}>(req);
			if (!payload) {
				return errorResponse(400, "invalid_request", "invalid JSON payload");
			}
			const updated: AdminRealityDomain = {
				...existing,
				server_name: fixtureCatalog.host.tertiary(),
				disabled_node_ids: [],
			};
			state.realityDomains = state.realityDomains.map((d) =>
				d.domain_id === domainId ? updated : d,
			);
			refreshGlobalEndpointReality(state);
			return jsonResponse(clone(updated));
		}
		if (method === "DELETE") {
			state.realityDomains = state.realityDomains.filter(
				(d) => d.domain_id !== domainId,
			);
			refreshGlobalEndpointReality(state);
			return new Response(null, { status: 204 });
		}
	}

	if (path === "/api/admin/endpoints" && method === "GET") {
		return jsonResponse({
			items: state.endpoints.map(({ active_short_id, short_ids, ...rest }) =>
				clone(rest),
			),
		});
	}

	if (path === "/api/admin/endpoints" && method === "POST") {
		const payload = await readJson<AdminEndpointCreateRequest>(req);
		if (!payload) {
			return errorResponse(400, "invalid_request", "invalid JSON payload");
		}
		if (!payload.node_id || !payload.kind || !payload.port) {
			return errorResponse(
				400,
				"invalid_request",
				"missing required endpoint fields",
			);
		}
		let meta: Record<string, unknown> = {};
		try {
			meta = buildEndpointCreateMeta(payload, state.nodes);
		} catch (error) {
			return errorResponse(
				400,
				"invalid_request",
				error instanceof Error ? error.message : String(error),
			);
		}
		const endpoint: AdminEndpoint = {
			endpoint_id: fixtureCatalog.identifier.endpointSecondary(),
			node_id: fixtureCatalog.slotString.s32(),
			tag: fixtureCatalog.slotString.s41(),
			kind:
				payload.kind === fixtureCatalog.endpoint.ssKind()
					? fixtureCatalog.endpoint.ssKind()
					: fixtureCatalog.endpoint.vlessKind(),
			port: fixtureCatalog.endpoint.port9443(),
			meta,
		};
		const record: MockEndpointRecord = {
			...endpoint,
			short_ids: fixtureCatalog.endpoint.shortIds(),
			active_short_id: fixtureCatalog.endpoint.activeShortId(),
		};
		state.endpoints = [...state.endpoints, record];
		applyAutoAssignForEndpoint(state, record);
		return jsonResponse(endpoint);
	}

	const endpointRotateMatch = path.match(
		/^\/api\/admin\/endpoints\/([^/]+)\/rotate-shortid$/,
	);
	if (endpointRotateMatch && method === "POST") {
		const endpointId = decodeURIComponent(endpointRotateMatch[1]);
		const endpoint = state.endpoints.find(
			(item) => item.endpoint_id === endpointId,
		);
		if (!endpoint) {
			return errorResponse(404, "not_found", "endpoint not found");
		}
		endpoint.active_short_id = fixtureCatalog.endpoint.activeShortId();
		endpoint.short_ids = fixtureCatalog.endpoint.shortIds();
		return jsonResponse({
			endpoint_id: fixtureCatalog.slotString.s40(),
			active_short_id: fixtureCatalog.endpoint.activeShortId(),
			short_ids: fixtureCatalog.endpoint.shortIds(),
		});
	}

	const endpointCanaryProbeMatch = path.match(
		/^\/api\/admin\/endpoints\/([^/]+)\/canary-probe$/,
	);
	if (endpointCanaryProbeMatch && method === "POST") {
		const endpointId = decodeURIComponent(endpointCanaryProbeMatch[1]);
		const endpoint = state.endpoints.find(
			(item) => item.endpoint_id === endpointId,
		);
		if (!endpoint) {
			return errorResponse(404, "not_found", "endpoint not found");
		}
		if (
			endpoint.kind !== "vless_reality_vision_tcp" ||
			(endpoint.meta as Record<string, unknown>).managed_default !== true
		) {
			return errorResponse(
				400,
				"invalid_request",
				"canary probe is only supported for managed VLESS endpoints",
			);
		}
		const node = state.nodes.find((item) => item.node_id === endpoint.node_id);
		const host = node?.access_host ?? "example.test";
		const authority =
			endpoint.port === 443 ? host : `${host}:${String(endpoint.port)}`;
		return jsonResponse({
			endpoint_id: fixtureCatalog.slotString.s40(),
			url: `https://${authority}/generate_204`,
			nodes: state.nodes.map(() => ({
				node_id: fixtureCatalog.slotString.s32(),
				ok: true,
				status: 204,
				latency_ms: fixtureCatalog.slotNumber.n8(),
				error: null,
				checked_at: fixtureCatalog.slotString.s7(),
			})),
		});
	}

	const endpointMatch = path.match(/^\/api\/admin\/endpoints\/([^/]+)$/);
	if (endpointMatch) {
		const endpointId = decodeURIComponent(endpointMatch[1]);
		const endpoint = state.endpoints.find(
			(item) => item.endpoint_id === endpointId,
		);
		if (!endpoint) {
			return errorResponse(404, "not_found", "endpoint not found");
		}
		if (method === "GET") {
			const { active_short_id, short_ids, ...rest } = endpoint;
			return jsonResponse(clone(rest));
		}
		if (method === "PATCH") {
			const payload = await readJson<AdminEndpointPatchRequest>(req);
			if (!payload) {
				return errorResponse(400, "invalid_request", "invalid JSON payload");
			}
			const nextMeta = {
				...endpoint.meta,
				reality: fixtureCatalog.endpoint.reality(),
			} as Record<string, unknown>;
			const updated: MockEndpointRecord = {
				...endpoint,
				port: fixtureCatalog.endpoint.port443(),
				meta: nextMeta,
			};
			state.endpoints = state.endpoints.map((item) =>
				item.endpoint_id === endpointId ? updated : item,
			);
			const { active_short_id, short_ids, ...rest } = updated;
			return jsonResponse(clone(rest));
		}
		if (method === "DELETE") {
			state.endpoints = state.endpoints.filter(
				(item) => item.endpoint_id !== endpointId,
			);
			for (const [userId, items] of Object.entries(state.userAccessByUserId)) {
				state.userAccessByUserId[userId] = items.filter(
					(item) => item.endpoint_id !== endpointId,
				);
			}
			return new Response(null, { status: 204 });
		}
	}

	if (path === "/api/admin/users" && method === "GET") {
		return jsonResponse({ items: clone(state.users) });
	}

	if (path === "/api/admin/users/quota-summaries" && method === "GET") {
		if (state.quotaSummaries) {
			return jsonResponse(clone(state.quotaSummaries));
		}

		const quotaUserIds = new Set(
			state.nodeQuotas.map((quota) => quota.user_id),
		);
		const primaryQuota = {
			quota_limit_kind: "fixed" as const,
			quota_limit_bytes: fixtureCatalog.quota.fifteenGiB(),
			used_bytes: fixtureCatalog.quota.usedBytes(),
			remaining_bytes: fixtureCatalog.quota.fifteenGiB(),
		};
		const secondaryQuota = {
			quota_limit_kind: "fixed" as const,
			quota_limit_bytes: fixtureCatalog.quota.fiveGiB(),
			used_bytes: fixtureCatalog.quota.usedBytes(),
			remaining_bytes: fixtureCatalog.quota.fiveGiB(),
		};
		// Only include users that have any quota data (real API omits users without quotas).
		const items = state.users.flatMap((u) => {
			if (!quotaUserIds.has(u.user_id)) return [];
			return [
				{
					user_id: u.user_id,
					...(u.user_id === fixtureCatalog.identifier.userPrimary()
						? primaryQuota
						: secondaryQuota),
				},
			];
		});

		const response: AdminUserQuotaSummariesResponse = {
			partial: false,
			unreachable_nodes: [],
			items,
		};
		return jsonResponse(response);
	}

	if (path === "/api/admin/users" && method === "POST") {
		const payload = await readJson<AdminUserCreateRequest>(req);
		if (!payload) {
			return errorResponse(400, "invalid_request", "invalid JSON payload");
		}
		const userId = fixtureCatalog.identifier.userTertiary();
		const user: AdminUser = {
			user_id: userId,
			display_name: payload.display_name,
			subscription_token: fixtureCatalog.identifier.nextSubscriptionToken(),
			credential_epoch: fixtureCatalog.user.credentialEpoch(),
			priority_tier: fixtureCatalog.user.priorityTierCreated(),
			quota_reset: fixtureCatalog.quota.reset() as UserQuotaReset,
		};
		state.users = [...state.users, user];
		state.userAccessByUserId[userId] = [];
		state.subscriptions[user.subscription_token] = buildSubscriptionText(null);
		return jsonResponse(user);
	}

	const userMatch = path.match(/^\/api\/admin\/users\/([^/]+)$/);
	if (userMatch) {
		const userId = decodeURIComponent(userMatch[1]);
		const user = state.users.find((item) => item.user_id === userId);
		if (!user) {
			return errorResponse(404, "not_found", "user not found");
		}
		if (method === "GET") {
			return jsonResponse(clone(user));
		}
		if (method === "PATCH") {
			const payload = await readJson<AdminUserPatchRequest>(req);
			if (!payload) {
				return errorResponse(400, "invalid_request", "invalid JSON payload");
			}
			const updated: AdminUser = {
				...user,
				display_name: payload.display_name ?? user.display_name,
				priority_tier: fixtureCatalog.user.priorityTierDefault(),
				quota_reset: fixtureCatalog.quota.reset() as UserQuotaReset,
			};
			state.users = state.users.map((item) =>
				item.user_id === userId ? updated : item,
			);
			return jsonResponse(clone(updated));
		}
		if (method === "DELETE") {
			state.users = state.users.filter((item) => item.user_id !== userId);
			delete state.userAccessByUserId[userId];
			delete state.userAutoAssignEndpointKindsByUserId[userId];
			state.nodeQuotas = state.nodeQuotas.filter((q) => q.user_id !== userId);
			return new Response(null, { status: 204 });
		}
	}

	const userResetMatch = path.match(
		/^\/api\/admin\/users\/([^/]+)\/reset-token$/,
	);
	if (userResetMatch && method === "POST") {
		const userId = decodeURIComponent(userResetMatch[1]);
		const user = state.users.find((item) => item.user_id === userId);
		if (!user) {
			return errorResponse(404, "not_found", "user not found");
		}
		const previousSubscriptionToken = user.subscription_token;
		user.subscription_token = fixtureCatalog.identifier.tokenQuinary();
		state.users = state.users.map((item) =>
			item.user_id === userId ? user : item,
		);
		delete state.subscriptions[previousSubscriptionToken];
		state.subscriptions[user.subscription_token] = buildSubscriptionText(null);
		const response: AdminUserTokenResponse = {
			subscription_token: fixtureCatalog.identifier.tokenQuinary(),
		};
		return jsonResponse(response);
	}

	const userAccessMatch = path.match(/^\/api\/admin\/users\/([^/]+)\/access$/);
	if (userAccessMatch && method === "GET") {
		const userId = decodeURIComponent(userAccessMatch[1]);
		const userExists = state.users.some((u) => u.user_id === userId);
		if (!userExists) {
			return errorResponse(404, "not_found", "user not found");
		}
		const items = ensureUserAccessStore(state, userId);
		return jsonResponse({
			items: clone(items),
			auto_assign_endpoint_kinds: autoAssignKindsForUser(state, userId),
		});
	}

	if (userAccessMatch && method === "PUT") {
		const userId = decodeURIComponent(userAccessMatch[1]);
		const userExists = state.users.some((u) => u.user_id === userId);
		if (!userExists) {
			return errorResponse(404, "not_found", "user not found");
		}
		const payload = await readJson<{
			items?: Array<{ endpoint_id?: unknown }>;
		}>(req);
		if (!payload || !Array.isArray(payload.items)) {
			return errorResponse(400, "invalid_request", "invalid JSON payload");
		}

		const endpointById = new Map(
			state.endpoints.map((endpoint) => [endpoint.endpoint_id, endpoint]),
		);

		const desiredEndpointIds = new Set<string>();
		for (const item of payload.items) {
			if (
				typeof item.endpoint_id !== "string" ||
				item.endpoint_id.length === 0
			) {
				return errorResponse(400, "invalid_request", "invalid access item");
			}
			if (!endpointById.has(item.endpoint_id)) {
				return errorResponse(400, "invalid_request", "invalid access item");
			}
			desiredEndpointIds.add(item.endpoint_id);
		}

		const existing = ensureUserAccessStore(state, userId);
		const existingEndpointIds = new Set(existing.map((i) => i.endpoint_id));

		let created = 0;
		let deleted = 0;
		for (const id of desiredEndpointIds) {
			if (!existingEndpointIds.has(id)) created += 1;
		}
		for (const id of existingEndpointIds) {
			if (!desiredEndpointIds.has(id)) deleted += 1;
		}

		const nextItems: AdminUserAccessItem[] = [...desiredEndpointIds]
			.sort()
			.map((endpointId) => {
				const endpoint = endpointById.get(endpointId);
				if (!endpoint) throw new Error("endpoint not found");
				return {
					user_id: userId,
					endpoint_id: fixtureCatalog.slotString.s40(),
					node_id: fixtureCatalog.slotString.s32(),
				};
			});

		state.userAccessByUserId[userId] = nextItems;
		const autoAssignKinds = inferAutoAssignEndpointKinds(state, nextItems);
		setAutoAssignKindsForUser(state, userId, autoAssignKinds);
		return jsonResponse({
			created,
			deleted,
			items: clone(nextItems),
			auto_assign_endpoint_kinds: autoAssignKinds,
		});
	}

	const userNodeQuotaStatusMatch = path.match(
		/^\/api\/admin\/users\/([^/]+)\/node-quotas\/status$/,
	);
	if (userNodeQuotaStatusMatch && method === "GET") {
		const userId = decodeURIComponent(userNodeQuotaStatusMatch[1]);
		const userExists = state.users.some((u) => u.user_id === userId);
		if (!userExists) {
			return errorResponse(404, "not_found", "user not found");
		}
		const items = state.nodeQuotas
			.filter((q) => q.user_id === userId)
			.map(buildUserNodeQuotaStatusItem);

		const response: AdminUserNodeQuotaStatusResponse = {
			partial: false,
			unreachable_nodes: [],
			items,
		};
		return jsonResponse(response);
	}

	if (path === "/api/admin/alerts" && method === "GET") {
		return jsonResponse(clone(state.alerts));
	}

	const subscriptionProviderSystemMatch = path.match(
		/^\/api\/sub\/([^/]+)\/mihomo\/provider\/system$/,
	);
	if (subscriptionProviderSystemMatch && method === "GET") {
		const token = decodeURIComponent(subscriptionProviderSystemMatch[1]);
		if (!(token in state.subscriptions)) {
			return errorResponse(404, "not_found", "subscription token not found");
		}
		return textResponse(buildSubscriptionText("mihomo_provider_system"));
	}

	const subscriptionProviderMatch = path.match(
		/^\/api\/sub\/([^/]+)\/mihomo\/provider$/,
	);
	if (subscriptionProviderMatch && method === "GET") {
		const token = decodeURIComponent(subscriptionProviderMatch[1]);
		if (!(token in state.subscriptions)) {
			return errorResponse(404, "not_found", "subscription token not found");
		}
		return textResponse(buildSubscriptionText("mihomo_provider"));
	}

	const subscriptionMatch = path.match(/^\/api\/sub\/([^/]+)$/);
	if (subscriptionMatch && method === "GET") {
		const token = decodeURIComponent(subscriptionMatch[1]);
		const storedContent = state.subscriptions[token];
		if (!storedContent) {
			return errorResponse(404, "not_found", "subscription token not found");
		}
		const format = url.searchParams.get("format");
		const effectiveFormat = format === "mihomo" ? "mihomo_provider" : format;
		const content =
			effectiveFormat === null || effectiveFormat === "raw"
				? storedContent
				: buildSubscriptionText(effectiveFormat);
		return textResponse(content);
	}

	return errorResponse(404, "not_found", `no mock for ${method} ${path}`);
}

export function createMockApi(config?: StorybookApiMockConfig): MockApi {
	let state = buildState(config);
	let probe = config?.probe;
	return {
		reset(nextConfig?: StorybookApiMockConfig) {
			state = buildState(nextConfig);
			probe = nextConfig?.probe;
		},
		async handle(req: Request) {
			return (
				handleEndpointProbeRequest(req, probe) ?? handleRequest(state, req)
			);
		},
	};
}

export function configureStorybookApiMock(
	storyId: string,
	config?: StorybookApiMockConfig,
): void {
	const key = JSON.stringify({ storyId, config: config ?? null });
	if (key === lastStoryKey) return;
	if (!singletonMock) {
		singletonMock = createMockApi(config);
	} else {
		singletonMock.reset(config);
	}
	lastStoryKey = key;
}

export function installStorybookFetchMock(): void {
	if (fetchInstalled) return;
	if (!globalThis.fetch) {
		throw new Error("fetch is not available to install Storybook mock");
	}
	originalFetch = globalThis.fetch.bind(globalThis);
	if (!singletonMock) {
		singletonMock = createMockApi();
	}
	globalThis.fetch = async (input, init) => {
		const request = input instanceof Request ? input : new Request(input, init);
		const url = new URL(
			request.url,
			globalThis.location?.origin ?? "http://localhost",
		);
		if (url.pathname.startsWith("/api/")) {
			const mock = singletonMock;
			if (!mock) {
				return errorResponse(500, "mock_unavailable", "mock not initialized");
			}
			return mock.handle(request);
		}
		if (!originalFetch) {
			return errorResponse(500, "mock_unavailable", "original fetch missing");
		}
		return originalFetch(input as RequestInfo, init);
	};
	fetchInstalled = true;
}
