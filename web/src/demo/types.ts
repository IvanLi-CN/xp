export type DemoRole = "admin" | "operator" | "viewer";

export type DemoScenarioId = "normal" | "incident" | "empty" | "large";

export type DemoNodeStatus = "healthy" | "degraded" | "offline";

export type DemoEndpointStatus = "serving" | "degraded" | "disabled";

export type DemoUserStatus = "active" | "quota_limited" | "disabled";

export type DemoNode = {
	id: string;
	name: string;
	region: string;
	accessHost: string;
	role: "leader" | "follower";
	term: number;
	status: DemoNodeStatus;
	latencyMs: number | null;
	quotaLimitGb: number | null;
	quotaUsedGb: number;
	lastSeenAt: string;
};

export type DemoEndpoint = {
	id: string;
	name: string;
	nodeId: string;
	kind: "vless_reality_vision_tcp" | "ss2022_2022_blake3_aes_128_gcm";
	port: number;
	status: DemoEndpointStatus;
	serverNames: string[];
	assignedUserIds: string[];
	probeLatencyMs: number | null;
	lastProbeAt: string | null;
	createdAt: string;
};

export type DemoUser = {
	id: string;
	displayName: string;
	email: string;
	locale: string;
	tier: "p1" | "p2" | "p3";
	status: DemoUserStatus;
	quotaLimitGb: number | null;
	quotaUsedGb: number;
	endpointIds: string[];
	subscriptionToken: string;
	createdAt: string;
};

export type DemoActivity = {
	id: string;
	at: string;
	kind: "success" | "warning" | "error" | "info";
	message: string;
};

export type DemoSession = {
	role: DemoRole;
	operatorName: string;
	startedAt: string;
};

export type DemoState = {
	scenarioId: DemoScenarioId;
	session: DemoSession | null;
	nodes: DemoNode[];
	endpoints: DemoEndpoint[];
	users: DemoUser[];
	activity: DemoActivity[];
	nextEndpoint: number;
	nextUser: number;
	lastDeletedUser: DemoUser | null;
};

export type DemoScenario = {
	id: DemoScenarioId;
	name: string;
	description: string;
	intent: string;
};
