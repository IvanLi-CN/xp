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
	DemoRole,
	DemoScenarioId,
	DemoSession,
	DemoState,
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
				>
			>;
	  }
	| { type: "deleteUser"; userId: string }
	| { type: "undoDeleteUser" };

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
			>
		>,
	) => void;
	deleteUser: (userId: string) => void;
	undoDeleteUser: () => void;
};

const DemoContext = createContext<DemoContextValue | null>(null);

function nowIso() {
	return new Date().toISOString();
}

function normalizeState(value: DemoState): DemoState {
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
		endpoints: value.endpoints.map((endpoint) => ({
			...endpoint,
			assignedUserIds: [...(endpointMembership.get(endpoint.id) ?? new Set())],
		})),
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
		createdAt: nowIso(),
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
