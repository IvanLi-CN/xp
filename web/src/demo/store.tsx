import {
	type ReactNode,
	createContext,
	useContext,
	useEffect,
	useMemo,
	useReducer,
} from "react";

import { createDemoState } from "./fixtures";
import {
	clearDemoFallbackState,
	getDemoStorageKey,
	hasDemoSession,
	isDemoSession,
	readDemoFallbackState,
	setDemoFallbackState,
	setPreferDemoFallbackState,
	shouldPreferDemoFallbackState,
} from "./session";
import type {
	DemoEndpoint,
	DemoProbeRun,
	DemoQuotaPolicy,
	DemoRealityDomain,
	DemoRole,
	DemoScenarioId,
	DemoServiceConfig,
	DemoSession,
	DemoState,
	DemoToolRun,
	DemoUser,
} from "./types";

export { clearDemoFallbackState, getDemoStorageKey, hasDemoSession };

type DemoEndpointInput = {
	name: string;
	nodeId: string;
	kind: DemoEndpoint["kind"];
	port: number;
	serverNames: string[];
};

type DemoUserInput = {
	displayName: string;
	email: string;
	locale: string;
	tier: DemoUser["tier"];
	quotaLimitGb: number | null;
	endpointIds: string[];
};

type DemoRealityDomainInput = {
	hostname: string;
	enabled: boolean;
	nodeIds: string[];
	notes: string;
};

type DemoAction =
	| { type: "replaceState"; state: DemoState }
	| {
			type: "login";
			role: DemoRole;
			operatorName: string;
			scenarioId: DemoScenarioId;
	  }
	| { type: "logout" }
	| { type: "resetScenario"; scenarioId: DemoScenarioId }
	| { type: "createEndpoint"; input: DemoEndpointInput }
	| {
			type: "updateEndpoint";
			endpointId: string;
			patch: Partial<
				Pick<DemoEndpoint, "name" | "port" | "status" | "serverNames">
			>;
	  }
	| {
			type: "completeProbe";
			endpointId: string;
			latencyMs: number;
			degraded: boolean;
	  }
	| { type: "createUser"; input: DemoUserInput }
	| {
			type: "updateUser";
			userId: string;
			patch: Partial<
				Pick<
					DemoUser,
					| "displayName"
					| "email"
					| "locale"
					| "tier"
					| "quotaLimitGb"
					| "endpointIds"
					| "mihomoMixinYaml"
					| "subscriptionToken"
				>
			>;
	  }
	| { type: "deleteUser"; userId: string }
	| { type: "undoDeleteUser" }
	| { type: "updateQuotaPolicy"; quotaPolicy: DemoQuotaPolicy }
	| { type: "createRealityDomain"; input: DemoRealityDomainInput }
	| {
			type: "updateRealityDomain";
			domainId: string;
			patch: Partial<
				Pick<DemoRealityDomain, "hostname" | "enabled" | "nodeIds" | "notes">
			>;
	  }
	| { type: "deleteRealityDomain"; domainId: string }
	| { type: "moveRealityDomain"; domainId: string; direction: "up" | "down" }
	| { type: "updateServiceConfig"; serviceConfig: DemoServiceConfig }
	| {
			type: "addToolRun";
			kind: DemoToolRun["kind"];
			status: DemoToolRun["status"];
			message: string;
	  }
	| { type: "createProbeRun"; run: DemoProbeRun };

type DemoContextValue = {
	state: DemoState;
	login: (input: {
		role: DemoRole;
		operatorName: string;
		scenarioId: DemoScenarioId;
	}) => void;
	logout: () => void;
	resetScenario: (scenarioId: DemoScenarioId) => void;
	createEndpoint: (input: DemoEndpointInput) => DemoEndpoint;
	updateEndpoint: (
		endpointId: string,
		patch: Partial<
			Pick<DemoEndpoint, "name" | "port" | "status" | "serverNames">
		>,
	) => void;
	completeProbe: (
		endpointId: string,
		latencyMs: number,
		degraded: boolean,
	) => void;
	createUser: (input: DemoUserInput) => DemoUser;
	updateUser: (
		userId: string,
		patch: Partial<
			Pick<
				DemoUser,
				| "displayName"
				| "email"
				| "locale"
				| "tier"
				| "quotaLimitGb"
				| "endpointIds"
				| "mihomoMixinYaml"
				| "subscriptionToken"
			>
		>,
	) => void;
	deleteUser: (userId: string) => void;
	undoDeleteUser: () => void;
	updateQuotaPolicy: (quotaPolicy: DemoQuotaPolicy) => void;
	createRealityDomain: (input: DemoRealityDomainInput) => DemoRealityDomain;
	updateRealityDomain: (
		domainId: string,
		patch: Partial<
			Pick<DemoRealityDomain, "hostname" | "enabled" | "nodeIds" | "notes">
		>,
	) => void;
	deleteRealityDomain: (domainId: string) => void;
	moveRealityDomain: (domainId: string, direction: "up" | "down") => void;
	updateServiceConfig: (serviceConfig: DemoServiceConfig) => void;
	addToolRun: (
		kind: DemoToolRun["kind"],
		status: DemoToolRun["status"],
		message: string,
	) => void;
	createProbeRun: (endpointId: string) => DemoProbeRun;
};

const DemoContext = createContext<DemoContextValue | null>(null);

function nowIso() {
	return new Date().toISOString();
}

function normalizeState(value: DemoState): DemoState {
	const defaults = createDemoState(value.scenarioId ?? "normal");
	const endpointMembership = new Map<string, Set<string>>();
	for (const endpoint of value.endpoints) {
		endpointMembership.set(endpoint.id, new Set());
	}
	for (const user of value.users) {
		for (const endpointId of user.endpointIds) {
			if (!endpointMembership.has(endpointId)) {
				endpointMembership.set(endpointId, new Set());
			}
			endpointMembership.get(endpointId)?.add(user.id);
		}
	}

	return {
		...value,
		session: isDemoSession(value.session) ? value.session : null,
		users: value.users.map((user) => ({
			...user,
			mihomoMixinYaml: user.mihomoMixinYaml ?? "",
		})),
		endpoints: value.endpoints.map((endpoint) => ({
			...endpoint,
			assignedUserIds: [...(endpointMembership.get(endpoint.id) ?? new Set())],
		})),
		realityDomains: value.realityDomains ?? defaults.realityDomains,
		quotaPolicy: value.quotaPolicy ?? defaults.quotaPolicy,
		serviceConfig: value.serviceConfig ?? defaults.serviceConfig,
		toolRuns: value.toolRuns ?? defaults.toolRuns,
		probeRuns: value.probeRuns ?? defaults.probeRuns,
		nextRealityDomain:
			typeof value.nextRealityDomain === "number"
				? value.nextRealityDomain
				: defaults.nextRealityDomain,
		nextToolRun:
			typeof value.nextToolRun === "number"
				? value.nextToolRun
				: defaults.nextToolRun,
		nextProbeRun:
			typeof value.nextProbeRun === "number"
				? value.nextProbeRun
				: defaults.nextProbeRun,
	};
}

function appendActivity(
	state: DemoState,
	kind: DemoState["activity"][number]["kind"],
	message: string,
): DemoActivityPatch {
	return {
		activity: [
			{
				id: `activity-${Date.now()}-${state.activity.length}`,
				at: nowIso(),
				kind,
				message,
			},
			...state.activity,
		].slice(0, 8),
	};
}

type DemoActivityPatch = Pick<DemoState, "activity">;

function buildEndpoint(
	state: DemoState,
	input: DemoEndpointInput,
): DemoEndpoint {
	const id = `endpoint-demo-${String(state.nextEndpoint).padStart(2, "0")}`;
	return {
		id,
		name: input.name.trim(),
		nodeId: input.nodeId,
		kind: input.kind,
		port: input.port,
		status: "serving",
		serverNames: input.serverNames,
		assignedUserIds: [],
		probeLatencyMs: null,
		lastProbeAt: null,
		createdAt: nowIso(),
	};
}

function buildUser(state: DemoState, input: DemoUserInput): DemoUser {
	const id = `user-demo-${String(state.nextUser).padStart(2, "0")}`;
	return {
		id,
		displayName: input.displayName.trim(),
		email: input.email.trim(),
		locale: input.locale.trim() || "en-US",
		tier: input.tier,
		status: "active",
		quotaLimitGb: input.quotaLimitGb,
		quotaUsedGb: 0,
		endpointIds: input.endpointIds,
		subscriptionToken: `sub_01HXPDEMO_CREATED_${String(state.nextUser).padStart(2, "0")}`,
		mihomoMixinYaml: "",
		createdAt: nowIso(),
	};
}

function buildRealityDomain(
	state: DemoState,
	input: DemoRealityDomainInput,
): DemoRealityDomain {
	const id = `domain-demo-${String(state.nextRealityDomain).padStart(2, "0")}`;
	return {
		id,
		hostname: input.hostname.trim().toLowerCase(),
		enabled: input.enabled,
		nodeIds: input.nodeIds,
		priority: state.realityDomains.length + 1,
		lastValidatedAt: null,
		notes: input.notes.trim(),
	};
}

function buildToolRun(
	state: DemoState,
	kind: DemoToolRun["kind"],
	status: DemoToolRun["status"],
	message: string,
): DemoToolRun {
	return {
		id: `tool-${String(state.nextToolRun).padStart(3, "0")}`,
		at: nowIso(),
		kind,
		status,
		message,
	};
}

function buildProbeRun(state: DemoState, endpointId: string): DemoProbeRun {
	const endpoint = state.endpoints.find((item) => item.id === endpointId);
	const startedAt = nowIso();
	const samples = state.nodes.map((node, index) => {
		if (!endpoint) {
			return {
				nodeId: node.id,
				status: "skipped" as const,
				latencyMs: null,
				message: "Endpoint was not found in the demo state.",
			};
		}
		if (node.status === "offline") {
			return {
				nodeId: node.id,
				status: "timeout" as const,
				latencyMs: null,
				message: "Node did not answer before the probe deadline.",
			};
		}
		if (endpoint.status === "disabled" && node.id === endpoint.nodeId) {
			return {
				nodeId: node.id,
				status: "timeout" as const,
				latencyMs: null,
				message: "Inbound is disabled on the owner node.",
			};
		}
		const base = node.latencyMs ?? 120;
		const endpointPenalty = endpoint.status === "degraded" ? 180 : 12;
		return {
			nodeId: node.id,
			status: "ok" as const,
			latencyMs: base + endpointPenalty + index * 7,
			message:
				node.id === endpoint.nodeId
					? "Owner node accepted the probe."
					: "Cross-node probe reached the endpoint.",
		};
	});
	const failed = samples.some((sample) => sample.status === "timeout");

	return {
		id: `probe-run-${String(state.nextProbeRun).padStart(3, "0")}`,
		endpointId,
		status: failed ? "failed" : "completed",
		startedAt,
		completedAt: nowIso(),
		samples,
	};
}

function reducer(state: DemoState, action: DemoAction): DemoState {
	if (action.type === "replaceState") {
		return action.state;
	}

	if (action.type === "login") {
		const session: DemoSession = {
			role: action.role,
			operatorName: action.operatorName.trim() || "Demo Operator",
			startedAt: nowIso(),
		};
		return {
			...createDemoState(action.scenarioId),
			session,
		};
	}

	if (action.type === "logout") {
		return { ...state, session: null };
	}

	if (action.type === "resetScenario") {
		return {
			...createDemoState(action.scenarioId),
			session: state.session,
		};
	}

	if (action.type === "createEndpoint") {
		const endpoint = buildEndpoint(state, action.input);
		return normalizeState({
			...state,
			endpoints: [...state.endpoints, endpoint],
			nextEndpoint: state.nextEndpoint + 1,
			...appendActivity(state, "success", `Endpoint ${endpoint.name} created.`),
		});
	}

	if (action.type === "updateEndpoint") {
		return {
			...state,
			endpoints: state.endpoints.map((endpoint) =>
				endpoint.id === action.endpointId
					? { ...endpoint, ...action.patch }
					: endpoint,
			),
			...appendActivity(
				state,
				"info",
				`Endpoint ${action.endpointId} updated.`,
			),
		};
	}

	if (action.type === "completeProbe") {
		return {
			...state,
			endpoints: state.endpoints.map((endpoint) =>
				endpoint.id === action.endpointId
					? {
							...endpoint,
							status: action.degraded ? "degraded" : "serving",
							probeLatencyMs: action.latencyMs,
							lastProbeAt: nowIso(),
						}
					: endpoint,
			),
			...appendActivity(
				state,
				action.degraded ? "warning" : "success",
				`Probe finished for ${action.endpointId}: ${action.latencyMs} ms.`,
			),
		};
	}

	if (action.type === "createUser") {
		const user = buildUser(state, action.input);
		return normalizeState({
			...state,
			users: [...state.users, user],
			nextUser: state.nextUser + 1,
			...appendActivity(state, "success", `User ${user.displayName} created.`),
		});
	}

	if (action.type === "updateUser") {
		return normalizeState({
			...state,
			users: state.users.map((user) =>
				user.id === action.userId ? { ...user, ...action.patch } : user,
			),
			...appendActivity(state, "info", `User ${action.userId} updated.`),
		});
	}

	if (action.type === "deleteUser") {
		const deleted =
			state.users.find((user) => user.id === action.userId) ?? null;
		return normalizeState({
			...state,
			users: state.users.filter((user) => user.id !== action.userId),
			lastDeletedUser: deleted,
			...appendActivity(
				state,
				"warning",
				deleted ? `User ${deleted.displayName} deleted.` : "User deleted.",
			),
		});
	}

	if (action.type === "undoDeleteUser") {
		if (!state.lastDeletedUser) return state;
		const restored = state.lastDeletedUser;
		return normalizeState({
			...state,
			users: [...state.users, restored],
			lastDeletedUser: null,
			...appendActivity(
				state,
				"success",
				`User ${restored.displayName} restored.`,
			),
		});
	}

	if (action.type === "updateQuotaPolicy") {
		return {
			...state,
			quotaPolicy: action.quotaPolicy,
			...appendActivity(state, "info", "Quota policy updated."),
		};
	}

	if (action.type === "createRealityDomain") {
		const domain = buildRealityDomain(state, action.input);
		return {
			...state,
			realityDomains: [...state.realityDomains, domain],
			nextRealityDomain: state.nextRealityDomain + 1,
			...appendActivity(
				state,
				"success",
				`Reality domain ${domain.hostname} created.`,
			),
		};
	}

	if (action.type === "updateRealityDomain") {
		return {
			...state,
			realityDomains: state.realityDomains.map((domain) =>
				domain.id === action.domainId
					? {
							...domain,
							...action.patch,
							lastValidatedAt:
								action.patch.hostname || action.patch.enabled !== undefined
									? nowIso()
									: domain.lastValidatedAt,
						}
					: domain,
			),
			...appendActivity(
				state,
				"info",
				`Reality domain ${action.domainId} updated.`,
			),
		};
	}

	if (action.type === "deleteRealityDomain") {
		return {
			...state,
			realityDomains: state.realityDomains
				.filter((domain) => domain.id !== action.domainId)
				.map((domain, index) => ({ ...domain, priority: index + 1 })),
			...appendActivity(
				state,
				"warning",
				`Reality domain ${action.domainId} deleted.`,
			),
		};
	}

	if (action.type === "moveRealityDomain") {
		const currentIndex = state.realityDomains.findIndex(
			(domain) => domain.id === action.domainId,
		);
		const targetIndex =
			action.direction === "up" ? currentIndex - 1 : currentIndex + 1;
		if (
			currentIndex < 0 ||
			targetIndex < 0 ||
			targetIndex >= state.realityDomains.length
		) {
			return state;
		}
		const nextDomains = [...state.realityDomains];
		const current = nextDomains[currentIndex];
		const target = nextDomains[targetIndex];
		if (!current || !target) return state;
		nextDomains[currentIndex] = target;
		nextDomains[targetIndex] = current;
		return {
			...state,
			realityDomains: nextDomains.map((domain, index) => ({
				...domain,
				priority: index + 1,
			})),
			...appendActivity(state, "info", "Reality domain order changed."),
		};
	}

	if (action.type === "updateServiceConfig") {
		return {
			...state,
			serviceConfig: action.serviceConfig,
			...appendActivity(state, "success", "Service config saved."),
		};
	}

	if (action.type === "addToolRun") {
		const run = buildToolRun(state, action.kind, action.status, action.message);
		return {
			...state,
			toolRuns: [run, ...state.toolRuns].slice(0, 8),
			nextToolRun: state.nextToolRun + 1,
			...appendActivity(
				state,
				action.status === "success" ? "success" : "error",
				action.message,
			),
		};
	}

	if (action.type === "createProbeRun") {
		const okSamples = action.run.samples.filter(
			(sample) => sample.status === "ok" && sample.latencyMs !== null,
		);
		const averageLatency =
			okSamples.length > 0
				? Math.round(
						okSamples.reduce(
							(sum, sample) => sum + (sample.latencyMs ?? 0),
							0,
						) / okSamples.length,
					)
				: null;
		return {
			...state,
			probeRuns: [action.run, ...state.probeRuns].slice(0, 12),
			nextProbeRun: state.nextProbeRun + 1,
			endpoints: state.endpoints.map((endpoint) =>
				endpoint.id === action.run.endpointId
					? {
							...endpoint,
							status: action.run.status === "failed" ? "degraded" : "serving",
							probeLatencyMs: averageLatency,
							lastProbeAt: action.run.completedAt,
						}
					: endpoint,
			),
			...appendActivity(
				state,
				action.run.status === "failed" ? "warning" : "success",
				`Probe run ${action.run.id} completed for ${action.run.endpointId}.`,
			),
		};
	}

	return state;
}

function readStoredState(): DemoState {
	const fallbackState = readDemoFallbackState();
	if (shouldPreferDemoFallbackState() && fallbackState) return fallbackState;

	try {
		const raw = localStorage.getItem(getDemoStorageKey());
		if (!raw) return createDemoState("normal");
		const parsed = JSON.parse(raw) as DemoState;
		if (
			!parsed ||
			!Array.isArray(parsed.nodes) ||
			!Array.isArray(parsed.users)
		) {
			return createDemoState("normal");
		}
		const normalized = normalizeState(parsed);
		setDemoFallbackState(normalized);
		return normalized;
	} catch {
		return readDemoFallbackState() ?? createDemoState("normal");
	}
}

function writeStoredState(state: DemoState): void {
	setDemoFallbackState(normalizeState(state));
	try {
		localStorage.setItem(getDemoStorageKey(), JSON.stringify(state));
		setPreferDemoFallbackState(false);
	} catch {
		setPreferDemoFallbackState(true);
	}
}

export function DemoProvider({ children }: { children: ReactNode }) {
	const [state, dispatch] = useReducer(reducer, undefined, readStoredState);

	useEffect(() => {
		writeStoredState(state);
	}, [state]);

	const value = useMemo<DemoContextValue>(
		() => ({
			state,
			login(input) {
				const session: DemoSession = {
					role: input.role,
					operatorName: input.operatorName.trim() || "Demo Operator",
					startedAt: nowIso(),
				};
				const nextState = {
					...createDemoState(input.scenarioId),
					session,
				};
				writeStoredState(nextState);
				dispatch({ type: "replaceState", state: nextState });
			},
			logout() {
				const nextState = { ...state, session: null };
				writeStoredState(nextState);
				dispatch({ type: "replaceState", state: nextState });
			},
			resetScenario(scenarioId) {
				dispatch({ type: "resetScenario", scenarioId });
			},
			createEndpoint(input) {
				const endpoint = buildEndpoint(state, input);
				dispatch({ type: "createEndpoint", input });
				return endpoint;
			},
			updateEndpoint(endpointId, patch) {
				dispatch({ type: "updateEndpoint", endpointId, patch });
			},
			completeProbe(endpointId, latencyMs, degraded) {
				dispatch({ type: "completeProbe", endpointId, latencyMs, degraded });
			},
			createUser(input) {
				const user = buildUser(state, input);
				dispatch({ type: "createUser", input });
				return user;
			},
			updateUser(userId, patch) {
				dispatch({ type: "updateUser", userId, patch });
			},
			deleteUser(userId) {
				dispatch({ type: "deleteUser", userId });
			},
			undoDeleteUser() {
				dispatch({ type: "undoDeleteUser" });
			},
			updateQuotaPolicy(quotaPolicy) {
				dispatch({ type: "updateQuotaPolicy", quotaPolicy });
			},
			createRealityDomain(input) {
				const domain = buildRealityDomain(state, input);
				dispatch({ type: "createRealityDomain", input });
				return domain;
			},
			updateRealityDomain(domainId, patch) {
				dispatch({ type: "updateRealityDomain", domainId, patch });
			},
			deleteRealityDomain(domainId) {
				dispatch({ type: "deleteRealityDomain", domainId });
			},
			moveRealityDomain(domainId, direction) {
				dispatch({ type: "moveRealityDomain", domainId, direction });
			},
			updateServiceConfig(serviceConfig) {
				dispatch({ type: "updateServiceConfig", serviceConfig });
			},
			addToolRun(kind, status, message) {
				dispatch({ type: "addToolRun", kind, status, message });
			},
			createProbeRun(endpointId) {
				const run = buildProbeRun(state, endpointId);
				dispatch({ type: "createProbeRun", run });
				return run;
			},
		}),
		[state],
	);

	return <DemoContext.Provider value={value}>{children}</DemoContext.Provider>;
}

export function useDemo() {
	const context = useContext(DemoContext);
	if (!context) {
		throw new Error("useDemo must be used within DemoProvider");
	}
	return context;
}
