import type {
	DemoActivity,
	DemoEndpoint,
	DemoNode,
	DemoScenario,
	DemoScenarioId,
	DemoState,
	DemoUser,
} from "./types";

export const DEMO_SCENARIOS: DemoScenario[] = [
	{
		id: "normal",
		name: "Normal operations",
		description:
			"Healthy cluster with active users, endpoints, and quota data.",
		intent: "Use this for the ordinary admin walkthrough.",
	},
	{
		id: "incident",
		name: "Partial outage",
		description:
			"One node is degraded and quota enforcement has blocked a user.",
		intent:
			"Use this to review error states, probe results, and recovery actions.",
	},
	{
		id: "empty",
		name: "Fresh install",
		description:
			"Cluster exists, but users and endpoints have not been created.",
		intent: "Use this to validate empty states and first-create flows.",
	},
	{
		id: "large",
		name: "Large tenant set",
		description:
			"Many users with mixed locales, long names, and quota pressure.",
		intent: "Use this to test search, filters, sorting, and pagination.",
	},
];

const baseNodes: DemoNode[] = [
	{
		id: "node-tokyo-1",
		name: "tokyo-1",
		region: "Tokyo",
		accessHost: "tokyo-1.edge.example.net",
		role: "leader",
		term: 42,
		status: "healthy",
		latencyMs: 18,
		quotaLimitGb: null,
		quotaUsedGb: 184,
		lastSeenAt: "2026-04-29T09:48:00Z",
	},
	{
		id: "node-osaka-1",
		name: "osaka-1",
		region: "Osaka",
		accessHost: "osaka-1.edge.example.net",
		role: "follower",
		term: 42,
		status: "healthy",
		latencyMs: 24,
		quotaLimitGb: 900,
		quotaUsedGb: 611,
		lastSeenAt: "2026-04-29T09:47:30Z",
	},
	{
		id: "node-sgp-1",
		name: "singapore-1",
		region: "Singapore",
		accessHost: "sgp-1.edge.example.net",
		role: "follower",
		term: 42,
		status: "healthy",
		latencyMs: 42,
		quotaLimitGb: 1200,
		quotaUsedGb: 433,
		lastSeenAt: "2026-04-29T09:46:54Z",
	},
];

const baseEndpoints: DemoEndpoint[] = [
	{
		id: "endpoint-tokyo-reality",
		name: "tokyo-reality-443",
		nodeId: "node-tokyo-1",
		kind: "vless_reality_vision_tcp",
		port: 443,
		status: "serving",
		serverNames: ["public.sn.files.1drv.com", "oneclient.sfx.ms"],
		assignedUserIds: ["user-lin", "user-ops"],
		probeLatencyMs: 31,
		lastProbeAt: "2026-04-29T09:42:00Z",
		createdAt: "2026-03-19T11:24:00Z",
	},
	{
		id: "endpoint-osaka-ss",
		name: "osaka-ss-8443",
		nodeId: "node-osaka-1",
		kind: "ss2022_2022_blake3_aes_128_gcm",
		port: 8443,
		status: "serving",
		serverNames: [],
		assignedUserIds: ["user-maria", "user-very-long"],
		probeLatencyMs: 54,
		lastProbeAt: "2026-04-29T09:37:00Z",
		createdAt: "2026-02-02T02:10:00Z",
	},
	{
		id: "endpoint-sgp-reality",
		name: "singapore-global-reality",
		nodeId: "node-sgp-1",
		kind: "vless_reality_vision_tcp",
		port: 443,
		status: "degraded",
		serverNames: ["public.bn.files.1drv.com"],
		assignedUserIds: ["user-sato"],
		probeLatencyMs: 118,
		lastProbeAt: "2026-04-29T09:31:00Z",
		createdAt: "2026-01-15T21:02:00Z",
	},
];

const baseUsers: DemoUser[] = [
	{
		id: "user-lin",
		displayName: "Lin Chen",
		email: "lin.chen@example.com",
		locale: "zh-CN",
		tier: "p1",
		status: "active",
		quotaLimitGb: 200,
		quotaUsedGb: 86,
		endpointIds: ["endpoint-tokyo-reality"],
		subscriptionToken: "sub_01HXPDEMO0LINCHEN8ZPMDV",
		createdAt: "2026-02-08T08:00:00Z",
	},
	{
		id: "user-maria",
		displayName: "Maria Alvarez",
		email: "maria.alvarez@example.com",
		locale: "es-MX",
		tier: "p2",
		status: "active",
		quotaLimitGb: 120,
		quotaUsedGb: 64,
		endpointIds: ["endpoint-osaka-ss"],
		subscriptionToken: "sub_01HXPDEMO0MARIA84J5T9",
		createdAt: "2026-02-12T16:25:00Z",
	},
	{
		id: "user-sato",
		displayName: "佐藤 未来",
		email: "sato.mirai@example.jp",
		locale: "ja-JP",
		tier: "p3",
		status: "quota_limited",
		quotaLimitGb: 80,
		quotaUsedGb: 81,
		endpointIds: ["endpoint-sgp-reality"],
		subscriptionToken: "sub_01HXPDEMO0SATO01EY84",
		createdAt: "2026-03-01T03:40:00Z",
	},
	{
		id: "user-very-long",
		displayName:
			"Operations reviewer with a very long display name that should truncate",
		email: "reviewer.long-name@example.co.uk",
		locale: "en-GB",
		tier: "p2",
		status: "active",
		quotaLimitGb: null,
		quotaUsedGb: 244,
		endpointIds: ["endpoint-osaka-ss"],
		subscriptionToken: "sub_01HXPDEMO_LONG_TOKEN_FOR_LAYOUT_REVIEW_9EY84",
		createdAt: "2026-03-18T10:30:00Z",
	},
	{
		id: "user-ops",
		displayName: "Ops break-glass account",
		email: "ops-breakglass@example.net",
		locale: "en-US",
		tier: "p1",
		status: "disabled",
		quotaLimitGb: 20,
		quotaUsedGb: 0,
		endpointIds: ["endpoint-tokyo-reality"],
		subscriptionToken: "sub_01HXPDEMO_DISABLED_OPS",
		createdAt: "2026-01-04T00:00:00Z",
	},
];

const baseActivity: DemoActivity[] = [
	{
		id: "activity-1",
		at: "2026-04-29T09:49:00Z",
		kind: "success",
		message: "tokyo-reality-443 probe returned 31 ms.",
	},
	{
		id: "activity-2",
		at: "2026-04-29T09:45:00Z",
		kind: "warning",
		message: "singapore-global-reality reported elevated latency.",
	},
	{
		id: "activity-3",
		at: "2026-04-29T09:41:00Z",
		kind: "info",
		message: "Lin Chen subscription token copied by operator.",
	},
];

function largeUsers(): DemoUser[] {
	const locales = ["zh-CN", "en-US", "ja-JP", "es-MX", "de-DE", "fr-FR"];
	return Array.from({ length: 28 }, (_, index) => {
		const tier = index % 7 === 0 ? "p1" : index % 3 === 0 ? "p2" : "p3";
		const limit = index % 5 === 0 ? null : 60 + (index % 8) * 20;
		const used = limit
			? Math.round(limit * (0.18 + (index % 6) * 0.11))
			: 90 + index;
		const status =
			limit && used >= limit
				? "quota_limited"
				: index % 13 === 0
					? "disabled"
					: "active";
		const endpointIds =
			index % 4 === 0
				? ["endpoint-tokyo-reality", "endpoint-osaka-ss"]
				: [
						baseEndpoints[index % baseEndpoints.length]?.id ??
							"endpoint-tokyo-reality",
					];
		return {
			id: `user-batch-${String(index + 1).padStart(2, "0")}`,
			displayName:
				index === 9
					? "A user name long enough to force truncation across dense tables"
					: `Demo Tenant ${String(index + 1).padStart(2, "0")}`,
			email: `tenant-${String(index + 1).padStart(2, "0")}@example.org`,
			locale: locales[index % locales.length] ?? "en-US",
			tier,
			status,
			quotaLimitGb: limit,
			quotaUsedGb: used,
			endpointIds,
			subscriptionToken: `sub_01HXPDEMOLARGE${String(index + 1).padStart(2, "0")}`,
			createdAt: `2026-03-${String((index % 24) + 1).padStart(2, "0")}T08:30:00Z`,
		};
	});
}

export function createDemoState(scenarioId: DemoScenarioId): DemoState {
	const nodes = baseNodes.map((node) => ({ ...node }));
	const endpoints = baseEndpoints.map((endpoint) => ({ ...endpoint }));
	const users = baseUsers.map((user) => ({ ...user }));
	let activity = baseActivity.map((item) => ({ ...item }));

	if (scenarioId === "incident") {
		nodes[1] = {
			...nodes[1],
			status: "degraded",
			latencyMs: 386,
			lastSeenAt: "2026-04-29T09:21:00Z",
		};
		nodes[2] = {
			...nodes[2],
			status: "offline",
			latencyMs: null,
			lastSeenAt: "2026-04-29T08:57:00Z",
		};
		endpoints[1] = {
			...endpoints[1],
			status: "degraded",
			probeLatencyMs: 640,
			lastProbeAt: "2026-04-29T09:23:00Z",
		};
		endpoints[2] = {
			...endpoints[2],
			status: "disabled",
			probeLatencyMs: null,
			lastProbeAt: "2026-04-29T09:22:00Z",
		};
		activity = [
			{
				id: "incident-1",
				at: "2026-04-29T09:26:00Z",
				kind: "error",
				message: "singapore-1 did not answer the last runtime probe.",
			},
			{
				id: "incident-2",
				at: "2026-04-29T09:24:00Z",
				kind: "warning",
				message: "osaka-ss-8443 latency crossed the 500 ms threshold.",
			},
			...activity,
		];
	}

	if (scenarioId === "empty") {
		return {
			scenarioId,
			session: null,
			nodes: [nodes[0] as DemoNode],
			endpoints: [],
			users: [],
			activity: [
				{
					id: "empty-1",
					at: "2026-04-29T09:00:00Z",
					kind: "info",
					message: "Cluster initialized. Create an endpoint, then add users.",
				},
			],
			nextEndpoint: 1,
			nextUser: 1,
			lastDeletedUser: null,
		};
	}

	if (scenarioId === "large") {
		const manyUsers = largeUsers();
		return {
			scenarioId,
			session: null,
			nodes,
			endpoints: endpoints.map((endpoint) => ({
				...endpoint,
				assignedUserIds: manyUsers
					.filter((user) => user.endpointIds.includes(endpoint.id))
					.map((user) => user.id),
			})),
			users: manyUsers,
			activity,
			nextEndpoint: 1,
			nextUser: manyUsers.length + 1,
			lastDeletedUser: null,
		};
	}

	return {
		scenarioId,
		session: null,
		nodes,
		endpoints,
		users,
		activity,
		nextEndpoint: 1,
		nextUser: 1,
		lastDeletedUser: null,
	};
}

export function getScenario(id: DemoScenarioId): DemoScenario {
	return (
		DEMO_SCENARIOS.find((scenario) => scenario.id === id) ?? DEMO_SCENARIOS[0]
	);
}
