import type { Page, Route } from "@playwright/test";

type QuotaResetSource = "user" | "node";

type UserQuotaReset =
	| { policy: "unlimited"; tz_offset_minutes: number }
	| {
			policy: "monthly";
			day_of_month: number;
			tz_offset_minutes: number;
	  };

type NodeQuotaReset =
	| { policy: "unlimited"; tz_offset_minutes?: number | null }
	| {
			policy: "monthly";
			day_of_month: number;
			tz_offset_minutes?: number | null;
	  };

type AdminUser = {
	user_id: string;
	display_name: string;
	subscription_token: string;
	quota_reset: UserQuotaReset;
};

type AdminNode = {
	node_id: string;
	node_name: string;
	api_base_url: string;
	access_host: string;
	quota_reset: NodeQuotaReset;
};

type AdminEndpoint = {
	endpoint_id: string;
	node_id: string;
	tag: string;
	kind: "vless_reality_vision_tcp" | "ss2022_2022_blake3_aes_128_gcm";
	port: number;
	meta: Record<string, unknown>;
};

type AdminUserNodeQuota = {
	user_id: string;
	node_id: string;
	quota_limit_bytes: number;
	quota_reset_source: QuotaResetSource;
};

type AdminUserGrant = {
	grant_id: string;
	user_id: string;
	endpoint_id: string;
	enabled: boolean;
	quota_limit_bytes: number;
	note: string | null;
	credentials: {
		vless?: { uuid: string; email: string };
		ss2022?: { method: string; password: string };
	};
};

type ClusterInfo = {
	cluster_id: string;
	node_id: string;
	role: string;
	leader_api_base_url: string;
	term: number;
};

type AlertsResponse = {
	partial: boolean;
	unreachable_nodes: string[];
	items: Array<{
		type: string;
		grant_id: string;
		endpoint_id: string;
		owner_node_id: string;
		desired_enabled: boolean;
		quota_banned: boolean;
		quota_banned_at: string | null;
		effective_enabled: boolean;
		message: string;
		action_hint: string;
	}>;
};

type MockApiOptions = {
	users?: AdminUser[];
	nodes?: AdminNode[];
	endpoints?: AdminEndpoint[];
	nodeQuotas?: AdminUserNodeQuota[];
	userGrantsByUserId?: Record<string, AdminUserGrant[]>;
	clusterInfo?: ClusterInfo;
	alerts?: AlertsResponse;
	healthStatus?: "ok" | "error";
	subscriptionContentRaw?: string;
	subscriptionContentClash?: string;
};

type MockState = {
	users: AdminUser[];
	nodes: AdminNode[];
	endpoints: AdminEndpoint[];
	nodeQuotas: AdminUserNodeQuota[];
	userGrantsByUserId: Record<string, AdminUserGrant[]>;
	clusterInfo: ClusterInfo;
	alerts: AlertsResponse;
	healthStatus: "ok" | "error";
	subscriptionContentRaw: string;
	subscriptionContentClash: string;
};

const defaultNodes: AdminNode[] = [
	{
		node_id: "node-1",
		node_name: "alpha",
		api_base_url: "http://127.0.0.1:62416",
		access_host: "alpha.example.com",
		quota_reset: {
			policy: "monthly",
			day_of_month: 1,
			tz_offset_minutes: null,
		},
	},
];

const defaultEndpoints: AdminEndpoint[] = [
	{
		endpoint_id: "endpoint-1",
		node_id: "node-1",
		tag: "edge-1",
		kind: "vless_reality_vision_tcp",
		port: 443,
		meta: {},
	},
];

const defaultUsers: AdminUser[] = [
	{
		user_id: "user-1",
		display_name: "Demo user",
		subscription_token: "sub-user-1",
		quota_reset: {
			policy: "monthly",
			day_of_month: 1,
			tz_offset_minutes: 480,
		},
	},
];

const defaultUserGrantsByUserId: Record<string, AdminUserGrant[]> = {
	"user-1": [
		{
			grant_id: "grant-1",
			user_id: "user-1",
			endpoint_id: "endpoint-1",
			enabled: true,
			quota_limit_bytes: 1_048_576,
			note: null,
			credentials: {
				vless: {
					uuid: "11111111-1111-1111-1111-111111111111",
					email: "demo@example.com",
				},
			},
		},
	],
};

const defaultClusterInfo: ClusterInfo = {
	cluster_id: "cluster-1",
	node_id: "node-1",
	role: "leader",
	leader_api_base_url: "http://127.0.0.1:62416",
	term: 1,
};

const defaultAlerts: AlertsResponse = {
	partial: false,
	unreachable_nodes: [],
	items: [],
};

const defaultSubscriptionClash = `proxies:
  - name: demo
    type: vless
    servername: example.com
    reality-opts:
      public-key: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
      short-id: 0123456789abcdef
`;

function jsonResponse(route: Route, payload: unknown, status = 200): void {
	void route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

function textResponse(route: Route, payload: string, status = 200): void {
	void route.fulfill({
		status,
		contentType: "text/plain",
		body: payload,
	});
}

function errorResponse(route: Route, message: string, status = 404): void {
	jsonResponse(
		route,
		{
			error: {
				code: "not_mocked",
				message,
				details: {},
			},
		},
		status,
	);
}

function parseJsonBody(request: { postData(): string | null }): Record<
	string,
	unknown
> {
	const raw = request.postData();
	if (!raw) return {};
	try {
		const parsed = JSON.parse(raw) as Record<string, unknown>;
		return parsed ?? {};
	} catch {
		return {};
	}
}

export async function setupApiMocks(
	page: Page,
	options: MockApiOptions = {},
): Promise<MockState> {
	const state: MockState = {
		users: options.users ? [...options.users] : [...defaultUsers],
		nodes: options.nodes ? [...options.nodes] : [...defaultNodes],
		endpoints: options.endpoints
			? [...options.endpoints]
			: [...defaultEndpoints],
		nodeQuotas: options.nodeQuotas ? [...options.nodeQuotas] : [],
		userGrantsByUserId: options.userGrantsByUserId
			? Object.fromEntries(
					Object.entries(options.userGrantsByUserId).map(([userId, grants]) => [
						userId,
						[...grants],
					]),
				)
			: Object.fromEntries(
					Object.entries(defaultUserGrantsByUserId).map(([userId, grants]) => [
						userId,
						[...grants],
					]),
				),
		clusterInfo: options.clusterInfo ?? { ...defaultClusterInfo },
		alerts: options.alerts ?? { ...defaultAlerts },
		healthStatus: options.healthStatus ?? "ok",
		subscriptionContentRaw:
			options.subscriptionContentRaw ?? "vless://example-host?encryption=none",
		subscriptionContentClash:
			options.subscriptionContentClash ?? defaultSubscriptionClash,
	};

	let userSeq = state.users.length + 1;
	let tokenSeq = 1;

	await page.route("**/api/**", async (route) => {
		const request = route.request();
		const url = new URL(request.url());
		const path = url.pathname;
		const method = request.method();
		if (!path.startsWith("/api/")) {
			void route.continue();
			return;
		}

		if (path === "/api/health" && method === "GET") {
			jsonResponse(route, { status: state.healthStatus });
			return;
		}

		if (path === "/api/cluster/info" && method === "GET") {
			jsonResponse(route, state.clusterInfo);
			return;
		}

		if (path === "/api/admin/alerts" && method === "GET") {
			jsonResponse(route, state.alerts);
			return;
		}

		if (path === "/api/admin/nodes" && method === "GET") {
			jsonResponse(route, { items: state.nodes });
			return;
		}

		if (path === "/api/admin/endpoints" && method === "GET") {
			jsonResponse(route, { items: state.endpoints });
			return;
		}

		if (path === "/api/admin/users" && method === "GET") {
			jsonResponse(route, { items: state.users });
			return;
		}

		if (path === "/api/admin/users" && method === "POST") {
			const payload = parseJsonBody(request);
			const displayName =
				typeof payload.display_name === "string"
					? payload.display_name
					: `User ${userSeq}`;
			const quotaReset = payload.quota_reset as UserQuotaReset | undefined;
			const userId = `user-${userSeq++}`;
			const newUser: AdminUser = {
				user_id: userId,
				display_name: displayName,
				subscription_token: `sub-${userId}`,
				quota_reset:
					quotaReset ??
					({
						policy: "monthly",
						day_of_month: 1,
						tz_offset_minutes: 480,
					} satisfies UserQuotaReset),
			};
			state.users.push(newUser);
			state.userGrantsByUserId[userId] = [];
			jsonResponse(route, newUser, 201);
			return;
		}

		const userGrantsMatch = path.match(
			/^\/api\/admin\/users\/([^/]+)\/grants$/,
		);
		if (userGrantsMatch && method === "GET") {
			const userId = decodeURIComponent(userGrantsMatch[1]);
			const user = state.users.find((item) => item.user_id === userId);
			if (!user) {
				errorResponse(route, `User not found: ${userId}`, 404);
				return;
			}
			const grants = (state.userGrantsByUserId[userId] ?? []).filter(
				(grant) => grant.enabled,
			);
			jsonResponse(route, { items: grants });
			return;
		}

		if (userGrantsMatch && method === "PUT") {
			const userId = decodeURIComponent(userGrantsMatch[1]);
			const user = state.users.find((item) => item.user_id === userId);
			if (!user) {
				errorResponse(route, `User not found: ${userId}`, 404);
				return;
			}

			const payload = parseJsonBody(request);
			const items = Array.isArray(payload.items) ? payload.items : null;
			if (!items) {
				errorResponse(route, "invalid grants payload", 400);
				return;
			}

			const endpointById = new Map(
				state.endpoints.map((endpoint) => [endpoint.endpoint_id, endpoint]),
			);
			const existing = state.userGrantsByUserId[userId] ?? [];
			const existingByEndpoint = new Map(
				existing.map((grant) => [grant.endpoint_id, grant]),
			);
			const nextByEndpoint = new Map<string, AdminUserGrant>();

			for (const item of items) {
				const endpointId =
					typeof item.endpoint_id === "string" ? item.endpoint_id : "";
				const enabled = Boolean(item.enabled);
				const quotaLimitBytes = Math.max(
					0,
					Math.floor(Number(item.quota_limit_bytes ?? 0)),
				);
				const note =
					typeof item.note === "string"
						? item.note
						: item.note === null
							? null
							: null;
				if (!endpointId) {
					errorResponse(route, "invalid endpoint_id", 400);
					return;
				}
				const endpoint = endpointById.get(endpointId);
				if (!endpoint) {
					errorResponse(route, `endpoint not found: ${endpointId}`, 404);
					return;
				}

				const prev = existingByEndpoint.get(endpointId);
				const grantIndex = tokenSeq++;
				nextByEndpoint.set(endpointId, {
					grant_id: prev?.grant_id ?? `grant-${grantIndex}-${endpointId}`,
					user_id: userId,
					endpoint_id: endpointId,
					enabled,
					quota_limit_bytes: quotaLimitBytes,
					note,
					credentials:
						prev?.credentials ??
						(endpoint.kind === "ss2022_2022_blake3_aes_128_gcm"
							? {
									ss2022: {
										method: "2022-blake3-aes-128-gcm",
										password: `mock-password-${grantIndex}`,
									},
								}
							: {
									vless: {
										uuid: "22222222-2222-2222-2222-222222222222",
										email: "mock@example.com",
									},
								}),
				});
			}

			const next = Array.from(nextByEndpoint.values());
			const created = next.filter(
				(grant) => !existingByEndpoint.has(grant.endpoint_id),
			).length;
			const deleted = existing.filter(
				(grant) => !nextByEndpoint.has(grant.endpoint_id),
			).length;
			const updated = next.filter((grant) => {
				const prev = existingByEndpoint.get(grant.endpoint_id);
				if (!prev) return false;
				return (
					prev.enabled !== grant.enabled ||
					prev.quota_limit_bytes !== grant.quota_limit_bytes ||
					(prev.note ?? null) !== (grant.note ?? null)
				);
			}).length;
			state.userGrantsByUserId[userId] = next;

			jsonResponse(route, {
				created,
				updated,
				deleted,
				items: next.filter((grant) => grant.enabled),
			});
			return;
		}

		const userNodeQuotasMatch = path.match(
			/^\/api\/admin\/users\/([^/]+)\/node-quotas$/,
		);
		if (userNodeQuotasMatch && method === "GET") {
			const userId = decodeURIComponent(userNodeQuotasMatch[1]);
			jsonResponse(route, {
				items: state.nodeQuotas.filter((q) => q.user_id === userId),
			});
			return;
		}

		const userNodeQuotaPutMatch = path.match(
			/^\/api\/admin\/users\/([^/]+)\/node-quotas\/([^/]+)$/,
		);
		if (userNodeQuotaPutMatch && method === "PUT") {
			const userId = decodeURIComponent(userNodeQuotaPutMatch[1]);
			const nodeId = decodeURIComponent(userNodeQuotaPutMatch[2]);
			const payload = parseJsonBody(request);
			const quotaLimitBytes = Number(payload.quota_limit_bytes ?? 0);
			const quotaResetSource: QuotaResetSource =
				payload.quota_reset_source === "node" ? "node" : "user";

			state.nodeQuotas = state.nodeQuotas.filter(
				(q) => !(q.user_id === userId && q.node_id === nodeId),
			);
			state.nodeQuotas.push({
				user_id: userId,
				node_id: nodeId,
				quota_limit_bytes: quotaLimitBytes,
				quota_reset_source: quotaResetSource,
			});
			jsonResponse(route, {
				user_id: userId,
				node_id: nodeId,
				quota_limit_bytes: quotaLimitBytes,
				quota_reset_source: quotaResetSource,
			});
			return;
		}

		if (path.startsWith("/api/admin/users/")) {
			const segments = path.split("/");
			const userId = decodeURIComponent(segments[4] ?? "");
			const isResetToken = segments[5] === "reset-token";

			if (isResetToken && method === "POST") {
				const user = state.users.find((item) => item.user_id === userId);
				if (!user) {
					errorResponse(route, `User not found: ${userId}`, 404);
					return;
				}
				const nextToken = `reset-${tokenSeq++}-${userId}`;
				user.subscription_token = nextToken;
				jsonResponse(route, { subscription_token: nextToken });
				return;
			}

			if (method === "GET") {
				const user = state.users.find((item) => item.user_id === userId);
				if (!user) {
					errorResponse(route, `User not found: ${userId}`, 404);
					return;
				}
				jsonResponse(route, user);
				return;
			}

			if (method === "PATCH") {
				const user = state.users.find((item) => item.user_id === userId);
				if (!user) {
					errorResponse(route, `User not found: ${userId}`, 404);
					return;
				}
				const payload = parseJsonBody(request);
				if (typeof payload.display_name === "string") {
					user.display_name = payload.display_name;
				}
				if (payload.quota_reset) {
					user.quota_reset = payload.quota_reset as UserQuotaReset;
				}
				jsonResponse(route, user);
				return;
			}

			if (method === "DELETE") {
				state.users = state.users.filter((item) => item.user_id !== userId);
				state.nodeQuotas = state.nodeQuotas.filter((q) => q.user_id !== userId);
				delete state.userGrantsByUserId[userId];
				void route.fulfill({ status: 204, body: "" });
				return;
			}
		}

		if (path.startsWith("/api/sub/") && method === "GET") {
			const format = url.searchParams.get("format");
			if (format === "clash") {
				textResponse(route, state.subscriptionContentClash);
				return;
			}
			textResponse(route, state.subscriptionContentRaw);
			return;
		}

		errorResponse(route, `${method} ${path} not mocked`);
	});

	return state;
}

export async function setAdminToken(
	page: Page,
	token = "test-token",
): Promise<void> {
	await page.addInitScript((value) => {
		window.localStorage.setItem("xp_admin_token", value as string);
	}, token);
}

export async function stubClipboard(page: Page): Promise<void> {
	await page.addInitScript(() => {
		// @ts-expect-error -- test-only helper
		window.__xp_clipboard_last_write = "";
		const clipboard = {
			writeText: async (text: string) => {
				// @ts-expect-error -- test-only helper
				window.__xp_clipboard_last_write = text;
			},
		};
		Object.defineProperty(navigator, "clipboard", {
			value: clipboard,
			configurable: true,
		});
	});
}
