import { fixtureCatalog } from "@/fixture-policy/catalog";
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
		id: fixtureCatalog.identifier.nodePrimary(),
		name: "tokyo-1",
		region: "Tokyo",
		accessHost: fixtureCatalog.host.primary(),
		apiBaseUrl: fixtureCatalog.url.primaryApi(),
		role: "leader",
		term: 42,
		status: "healthy",
		latencyMs: fixtureCatalog.metric.latencyLow(),
		quotaLimitGb: null,
		quotaUsedGb: 184,
		lastSeenAt: fixtureCatalog.timestamp.recent(),
	},
	{
		id: fixtureCatalog.identifier.nodeSecondary(),
		name: "osaka-1",
		region: "Osaka",
		accessHost: fixtureCatalog.host.secondary(),
		apiBaseUrl: fixtureCatalog.url.secondaryApi(),
		role: "follower",
		term: 42,
		status: "healthy",
		latencyMs: fixtureCatalog.metric.latencyLow(),
		quotaLimitGb: 900,
		quotaUsedGb: 611,
		lastSeenAt: fixtureCatalog.timestamp.baseline(),
	},
	{
		id: fixtureCatalog.identifier.nodeTertiary(),
		name: "singapore-1",
		region: "Singapore",
		accessHost: fixtureCatalog.host.tertiary(),
		apiBaseUrl: fixtureCatalog.url.tertiaryApi(),
		role: "follower",
		term: 42,
		status: "healthy",
		latencyMs: fixtureCatalog.metric.latencyHigh(),
		quotaLimitGb: 1200,
		quotaUsedGb: 433,
		lastSeenAt: fixtureCatalog.timestamp.baseline(),
	},
];

const DEMO_LOCAL_NODE_ID = "node-tokyo-1";

const baseEndpoints: DemoEndpoint[] = [
	{
		id: fixtureCatalog.identifier.endpointPrimary(),
		name: "tokyo-reality-443",
		nodeId: fixtureCatalog.identifier.nodePrimary(),
		kind: "vless_reality_vision_tcp",
		port: 443,
		status: "serving",
		serverNames: fixtureCatalog.list.primaryServerNames(),
		managedDefault: true,
		canaryUpstreamUrl: fixtureCatalog.url.loopback39043(),
		canaryUpstreamMode: "auto",
		acceptedAuthorities: fixtureCatalog.list.primaryAuthorities(),
		assignedUserIds: [
			fixtureCatalog.identifier.userPrimary(),
			fixtureCatalog.identifier.userQuinary(),
		],
		probeLatencyMs: fixtureCatalog.metric.latencyLow(),
		lastProbeAt: fixtureCatalog.timestamp.recent(),
		createdAt: fixtureCatalog.timestamp.baseline(),
	},
	{
		id: fixtureCatalog.identifier.endpointSecondary(),
		name: "osaka-ss-8443",
		nodeId: fixtureCatalog.identifier.nodeSecondary(),
		kind: "ss2022_2022_blake3_aes_128_gcm",
		port: 8443,
		status: "serving",
		serverNames: fixtureCatalog.list.secondaryServerNames(),
		assignedUserIds: [
			fixtureCatalog.identifier.userSecondary(),
			fixtureCatalog.identifier.userQuaternary(),
		],
		probeLatencyMs: fixtureCatalog.metric.latencyHigh(),
		lastProbeAt: fixtureCatalog.timestamp.baseline(),
		createdAt: fixtureCatalog.timestamp.baseline(),
	},
	{
		id: fixtureCatalog.identifier.endpointTertiary(),
		name: "singapore-global-reality",
		nodeId: fixtureCatalog.identifier.nodeTertiary(),
		kind: "vless_reality_vision_tcp",
		port: 443,
		status: "degraded",
		serverNames: fixtureCatalog.list.tertiaryServerNames(),
		managedDefault: true,
		canaryUpstreamUrl: fixtureCatalog.url.loopback39043(),
		canaryUpstreamMode: "auto",
		acceptedAuthorities: fixtureCatalog.list.tertiaryAuthorities(),
		assignedUserIds: [fixtureCatalog.identifier.userTertiary()],
		probeLatencyMs: fixtureCatalog.metric.latencyHigh(),
		lastProbeAt: fixtureCatalog.timestamp.baseline(),
		createdAt: fixtureCatalog.timestamp.baseline(),
	},
];

const baseUsers: DemoUser[] = [
	{
		id: fixtureCatalog.identifier.userPrimary(),
		displayName: "Lin Chen",
		email: "lin.chen@example.com",
		locale: "zh-CN",
		tier: "p1",
		status: "active",
		quotaLimitGb: 200,
		quotaUsedGb: 86,
		endpointIds: [fixtureCatalog.identifier.endpointPrimary()],
		subscriptionToken: fixtureCatalog.identifier.tokenPrimary(),
		mihomoMixinYaml:
			"rules:\n  - DOMAIN-SUFFIX,example.net,DIRECT\n  - GEOIP,CN,DIRECT\n",
		createdAt: fixtureCatalog.timestamp.baseline(),
	},
	{
		id: fixtureCatalog.identifier.userSecondary(),
		displayName: "Maria Alvarez",
		email: "maria.alvarez@example.com",
		locale: "es-MX",
		tier: "p2",
		status: "active",
		quotaLimitGb: 120,
		quotaUsedGb: 64,
		endpointIds: [fixtureCatalog.identifier.endpointSecondary()],
		subscriptionToken: fixtureCatalog.identifier.tokenSecondary(),
		mihomoMixinYaml: "proxy-groups:\n  - name: Auto\n    type: url-test\n",
		createdAt: fixtureCatalog.timestamp.baseline(),
	},
	{
		id: fixtureCatalog.identifier.userTertiary(),
		displayName: "佐藤 未来",
		email: "sato.mirai@example.jp",
		locale: "ja-JP",
		tier: "p3",
		status: "quota_limited",
		quotaLimitGb: 80,
		quotaUsedGb: 81,
		endpointIds: [fixtureCatalog.identifier.endpointTertiary()],
		subscriptionToken: fixtureCatalog.identifier.tokenTertiary(),
		mihomoMixinYaml: "rules:\n  - MATCH,Proxy\n",
		createdAt: fixtureCatalog.timestamp.recent(),
	},
	{
		id: fixtureCatalog.identifier.userQuaternary(),
		displayName:
			"Operations reviewer with a very long display name that should truncate",
		email: "reviewer.long-name@example.co.uk",
		locale: "en-GB",
		tier: "p2",
		status: "active",
		quotaLimitGb: null,
		quotaUsedGb: 244,
		endpointIds: [fixtureCatalog.identifier.endpointSecondary()],
		subscriptionToken: fixtureCatalog.identifier.tokenQuaternary(),
		mihomoMixinYaml: "dns:\n  enable: true\n  enhanced-mode: fake-ip\n",
		createdAt: fixtureCatalog.timestamp.recent(),
	},
	{
		id: fixtureCatalog.identifier.userQuinary(),
		displayName: "Ops break-glass account",
		email: "ops-breakglass@example.net",
		locale: "en-US",
		tier: "p1",
		status: "disabled",
		quotaLimitGb: 20,
		quotaUsedGb: 0,
		endpointIds: [fixtureCatalog.identifier.endpointPrimary()],
		subscriptionToken: fixtureCatalog.identifier.tokenQuinary(),
		mihomoMixinYaml: "",
		createdAt: fixtureCatalog.timestamp.baseline(),
	},
];

const baseRealityDomains: DemoRealityDomain[] = [
	{
		id: "domain-onedrive",
		hostname: fixtureCatalog.host.serverPrimary(),
		enabled: true,
		nodeIds: [
			fixtureCatalog.identifier.nodePrimary(),
			fixtureCatalog.identifier.nodeSecondary(),
		],
		priority: 1,
		lastValidatedAt: fixtureCatalog.timestamp.recent(),
		notes: "Primary Reality camouflage domain for Japan nodes.",
	},
	{
		id: "domain-office",
		hostname: fixtureCatalog.host.serverSecondary(),
		enabled: true,
		nodeIds: [fixtureCatalog.identifier.nodePrimary()],
		priority: 2,
		lastValidatedAt: fixtureCatalog.timestamp.none(),
		notes: "Secondary serverName for token rotation tests.",
	},
	{
		id: "domain-archive",
		hostname: fixtureCatalog.host.serverSecondary(),
		enabled: false,
		nodeIds: [fixtureCatalog.identifier.nodeTertiary()],
		priority: 3,
		lastValidatedAt: fixtureCatalog.timestamp.baseline(),
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
		[fixtureCatalog.identifier.nodePrimary()]: 120,
		[fixtureCatalog.identifier.nodeSecondary()]: 90,
		[fixtureCatalog.identifier.nodeTertiary()]: 70,
	},
};

const baseServiceConfig: DemoServiceConfig = {
	publicOrigin: fixtureCatalog.url.publicOrigin(),
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
		endpointId: fixtureCatalog.identifier.endpointPrimary(),
		status: "completed",
		startedAt: fixtureCatalog.timestamp.baseline(),
		completedAt: fixtureCatalog.timestamp.recent(),
		samples: [
			{
				nodeId: fixtureCatalog.identifier.nodePrimary(),
				status: "ok",
				latencyMs: fixtureCatalog.metric.latencyLow(),
				message: "Inbound accepted the probe.",
			},
			{
				nodeId: fixtureCatalog.identifier.nodeSecondary(),
				status: "ok",
				latencyMs: fixtureCatalog.metric.latencyHigh(),
				message: "Cross-node probe succeeded.",
			},
			{
				nodeId: fixtureCatalog.identifier.nodeTertiary(),
				status: "ok",
				latencyMs: fixtureCatalog.metric.latencyHigh(),
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
				? [
						fixtureCatalog.identifier.endpointPrimary(),
						fixtureCatalog.identifier.endpointSecondary(),
					]
				: [
						baseEndpoints[index % baseEndpoints.length]?.id ??
							fixtureCatalog.identifier.endpointPrimary(),
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
			subscriptionToken: fixtureCatalog.identifier.tokenPrimary(),
			mihomoMixinYaml:
				index % 4 === 0 ? "rules:\n  - DOMAIN-SUFFIX,internal,DIRECT\n" : "",
			createdAt: fixtureCatalog.timestamp.baseline(),
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
			latencyMs: fixtureCatalog.metric.latencyHigh(),
			lastSeenAt: fixtureCatalog.timestamp.baseline(),
		};
		nodes[2] = {
			...nodes[2],
			status: "offline",
			latencyMs: fixtureCatalog.metric.none(),
			lastSeenAt: fixtureCatalog.timestamp.baseline(),
		};
		endpoints[1] = {
			...endpoints[1],
			status: "degraded",
			probeLatencyMs: fixtureCatalog.metric.latencyHigh(),
			lastProbeAt: fixtureCatalog.timestamp.baseline(),
		};
		endpoints[2] = {
			...endpoints[2],
			status: "disabled",
			probeLatencyMs: fixtureCatalog.metric.none(),
			lastProbeAt: fixtureCatalog.timestamp.baseline(),
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
			localNodeId: DEMO_LOCAL_NODE_ID,
			nodes: [nodes[0] as DemoNode],
			endpoints: [],
			users: [],
			realityDomains: cloneRealityDomains().slice(0, 1),
			quotaPolicy: {
				...cloneQuotaPolicy(),
				nodeWeights: { [fixtureCatalog.identifier.nodePrimary()]: 100 },
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
			localNodeId: DEMO_LOCAL_NODE_ID,
			nodes: nodes.map((node) =>
				node.id === "node-sgp-1"
					? {
							...node,
							name: "singapore-edge-with-an-intentionally-long-hostname",
						}
					: node,
			),
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
		localNodeId: DEMO_LOCAL_NODE_ID,
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
