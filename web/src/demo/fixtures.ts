import type {
	DemoActivity,
	DemoEndpoint,
	DemoNode,
	DemoProbeRun,
	DemoQuotaPolicy,
	DemoRealityDomain,
	DemoScenario,
	DemoScenarioId,
	DemoServiceConfig,
	DemoState,
	DemoToolRun,
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
		mihomoMixinYaml:
			"rules:\n  - DOMAIN-SUFFIX,example.net,DIRECT\n  - GEOIP,CN,DIRECT\n",
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
		mihomoMixinYaml: "proxy-groups:\n  - name: Auto\n    type: url-test\n",
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
		mihomoMixinYaml: "rules:\n  - MATCH,Proxy\n",
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
		mihomoMixinYaml: "dns:\n  enable: true\n  enhanced-mode: fake-ip\n",
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
		mihomoMixinYaml: "",
		createdAt: "2026-01-04T00:00:00Z",
	},
];

const baseRealityDomains: DemoRealityDomain[] = [
	{
		id: "domain-onedrive",
		hostname: "public.sn.files.1drv.com",
		enabled: true,
		nodeIds: ["node-tokyo-1", "node-osaka-1"],
		priority: 1,
		lastValidatedAt: "2026-04-29T09:32:00Z",
		notes: "Primary Reality camouflage domain for Japan nodes.",
	},
	{
		id: "domain-office",
		hostname: "oneclient.sfx.ms",
		enabled: true,
		nodeIds: ["node-tokyo-1"],
		priority: 2,
		lastValidatedAt: "2026-04-29T08:52:00Z",
		notes: "Secondary serverName for token rotation tests.",
	},
	{
		id: "domain-archive",
		hostname: "public.bn.files.1drv.com",
		enabled: false,
		nodeIds: ["node-sgp-1"],
		priority: 3,
		lastValidatedAt: null,
		notes: "Disabled while Singapore probe is degraded.",
	},
];

const baseQuotaPolicy: DemoQuotaPolicy = {
	defaultLimitGb: 160,
	resetPolicy: "monthly",
	enforcementMode: "block",
	tierWeights: {
		p1: 160,
		p2: 100,
		p3: 60,
	},
	nodeWeights: {
		"node-tokyo-1": 120,
		"node-osaka-1": 90,
		"node-sgp-1": 70,
	},
};

const baseServiceConfig: DemoServiceConfig = {
	publicOrigin: "https://tokio-xp.example.net",
	defaultSubscriptionFormat: "mihomo",
	mihomoDelivery: "provider",
	auditLogRetentionDays: 30,
	xrayRestartStrategy: "rolling",
};

const baseToolRuns: DemoToolRun[] = [
	{
		id: "tool-1",
		at: "2026-04-29T09:18:00Z",
		kind: "mihomo_redact",
		status: "success",
		message: "Redacted 2 subscription tokens and 1 server address.",
	},
];

const baseProbeRuns: DemoProbeRun[] = [
	{
		id: "probe-run-001",
		endpointId: "endpoint-tokyo-reality",
		status: "completed",
		startedAt: "2026-04-29T09:41:42Z",
		completedAt: "2026-04-29T09:42:00Z",
		samples: [
			{
				nodeId: "node-tokyo-1",
				status: "ok",
				latencyMs: 31,
				message: "Inbound accepted the probe.",
			},
			{
				nodeId: "node-osaka-1",
				status: "ok",
				latencyMs: 44,
				message: "Cross-node probe succeeded.",
			},
			{
				nodeId: "node-sgp-1",
				status: "ok",
				latencyMs: 71,
				message: "Cross-region path is slower but healthy.",
			},
		],
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
			mihomoMixinYaml:
				index % 4 === 0 ? "rules:\n  - DOMAIN-SUFFIX,internal,DIRECT\n" : "",
			createdAt: `2026-03-${String((index % 24) + 1).padStart(2, "0")}T08:30:00Z`,
		};
	});
}

function cloneRealityDomains(): DemoRealityDomain[] {
	return baseRealityDomains.map((domain) => ({
		...domain,
		nodeIds: [...domain.nodeIds],
	}));
}

function cloneQuotaPolicy(): DemoQuotaPolicy {
	return {
		...baseQuotaPolicy,
		tierWeights: { ...baseQuotaPolicy.tierWeights },
		nodeWeights: { ...baseQuotaPolicy.nodeWeights },
	};
}

function cloneServiceConfig(): DemoServiceConfig {
	return { ...baseServiceConfig };
}

function cloneToolRuns(): DemoToolRun[] {
	return baseToolRuns.map((run) => ({ ...run }));
}

function cloneProbeRuns(): DemoProbeRun[] {
	return baseProbeRuns.map((run) => ({
		...run,
		samples: run.samples.map((sample) => ({ ...sample })),
	}));
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
			realityDomains: cloneRealityDomains().slice(0, 1),
			quotaPolicy: {
				...cloneQuotaPolicy(),
				nodeWeights: { "node-tokyo-1": 100 },
			},
			serviceConfig: cloneServiceConfig(),
			toolRuns: [],
			probeRuns: [],
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
			nextRealityDomain: 1,
			nextToolRun: 1,
			nextProbeRun: 1,
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
			realityDomains: cloneRealityDomains(),
			quotaPolicy: cloneQuotaPolicy(),
			serviceConfig: cloneServiceConfig(),
			toolRuns: cloneToolRuns(),
			probeRuns: cloneProbeRuns(),
			activity,
			nextEndpoint: 1,
			nextUser: manyUsers.length + 1,
			nextRealityDomain: 1,
			nextToolRun: 2,
			nextProbeRun: 2,
			lastDeletedUser: null,
		};
	}

	return {
		scenarioId,
		session: null,
		nodes,
		endpoints,
		users,
		realityDomains: cloneRealityDomains(),
		quotaPolicy: cloneQuotaPolicy(),
		serviceConfig: cloneServiceConfig(),
		toolRuns: cloneToolRuns(),
		probeRuns: cloneProbeRuns(),
		activity,
		nextEndpoint: 1,
		nextUser: 1,
		nextRealityDomain: 1,
		nextToolRun: 2,
		nextProbeRun: 2,
		lastDeletedUser: null,
	};
}

export function getScenario(id: DemoScenarioId): DemoScenario {
	return (
		DEMO_SCENARIOS.find((scenario) => scenario.id === id) ?? DEMO_SCENARIOS[0]
	);
}
