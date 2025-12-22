import { describe, expect, it } from "vitest";

import {
	AdminEndpointRotateResponseSchema,
	AdminEndpointSchema,
	AdminEndpointsResponseSchema,
} from "./adminEndpoints";
import {
	AdminGrantUsageResponseSchema,
	AdminGrantsResponseSchema,
} from "./adminGrants";
import { AdminJoinTokenResponseSchema } from "./adminJoinTokens";
import { AdminNodesResponseSchema } from "./adminNodes";
import {
	AdminUserTokenResponseSchema,
	AdminUsersResponseSchema,
} from "./adminUsers";
import { BackendErrorResponseSchema } from "./backendError";
import { ClusterInfoResponseSchema } from "./clusterInfo";

describe("BackendErrorResponseSchema", () => {
	it("accepts { error: { code, message, details } }", () => {
		expect(
			BackendErrorResponseSchema.parse({
				error: { code: "unauthorized", message: "nope", details: {} },
			}),
		).toEqual({
			error: { code: "unauthorized", message: "nope", details: {} },
		});
	});
});

describe("ClusterInfoResponseSchema", () => {
	it("accepts expected shape", () => {
		expect(
			ClusterInfoResponseSchema.parse({
				cluster_id: "01HZXKQF2Z6C8W8E9Y5C8M0X8Q",
				node_id: "01HZXKQF2Z6C8W8E9Y5C8M0X8Q",
				role: "leader",
				leader_api_base_url: "https://127.0.0.1:62416",
				term: 1,
			}),
		).toEqual({
			cluster_id: "01HZXKQF2Z6C8W8E9Y5C8M0X8Q",
			node_id: "01HZXKQF2Z6C8W8E9Y5C8M0X8Q",
			role: "leader",
			leader_api_base_url: "https://127.0.0.1:62416",
			term: 1,
		});
	});
});

describe("AdminNodesResponseSchema", () => {
	it("accepts expected shape", () => {
		expect(
			AdminNodesResponseSchema.parse({
				items: [
					{
						node_id: "01HZXKQF2Z6C8W8E9Y5C8M0X8Q",
						node_name: "node-1",
						api_base_url: "https://127.0.0.1:62416",
						public_domain: "",
					},
				],
			}),
		).toEqual({
			items: [
				{
					node_id: "01HZXKQF2Z6C8W8E9Y5C8M0X8Q",
					node_name: "node-1",
					api_base_url: "https://127.0.0.1:62416",
					public_domain: "",
				},
			],
		});
	});
});

describe("AdminEndpointSchema", () => {
	it("accepts expected shape", () => {
		expect(
			AdminEndpointSchema.parse({
				endpoint_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
				node_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
				tag: "vless-vision-01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
				kind: "vless_reality_vision_tcp",
				port: 443,
				meta: {},
			}),
		).toEqual({
			endpoint_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
			node_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
			tag: "vless-vision-01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
			kind: "vless_reality_vision_tcp",
			port: 443,
			meta: {},
		});
	});
});

describe("AdminEndpointsResponseSchema", () => {
	it("accepts expected shape", () => {
		expect(
			AdminEndpointsResponseSchema.parse({
				items: [
					{
						endpoint_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
						node_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
						tag: "vless-vision-01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
						kind: "vless_reality_vision_tcp",
						port: 443,
						meta: {},
					},
				],
			}),
		).toEqual({
			items: [
				{
					endpoint_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
					node_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
					tag: "vless-vision-01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
					kind: "vless_reality_vision_tcp",
					port: 443,
					meta: {},
				},
			],
		});
	});
});

describe("AdminEndpointRotateResponseSchema", () => {
	it("accepts expected shape", () => {
		expect(
			AdminEndpointRotateResponseSchema.parse({
				endpoint_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
				active_short_id: "0123456789abcdef",
				short_ids: ["0123456789abcdef", "0123456789abcdff"],
			}),
		).toEqual({
			endpoint_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
			active_short_id: "0123456789abcdef",
			short_ids: ["0123456789abcdef", "0123456789abcdff"],
		});
	});
});

describe("AdminUsersResponseSchema", () => {
	it("accepts expected shape", () => {
		expect(
			AdminUsersResponseSchema.parse({
				items: [
					{
						user_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
						display_name: "alice",
						subscription_token: "sub_123",
						cycle_policy_default: "by_user",
						cycle_day_of_month_default: 1,
					},
				],
			}),
		).toEqual({
			items: [
				{
					user_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
					display_name: "alice",
					subscription_token: "sub_123",
					cycle_policy_default: "by_user",
					cycle_day_of_month_default: 1,
				},
			],
		});
	});
});

describe("AdminUserTokenResponseSchema", () => {
	it("accepts expected shape", () => {
		expect(
			AdminUserTokenResponseSchema.parse({
				subscription_token: "sub_123",
			}),
		).toEqual({
			subscription_token: "sub_123",
		});
	});
});

describe("AdminGrantsResponseSchema", () => {
	it("accepts expected shape", () => {
		expect(
			AdminGrantsResponseSchema.parse({
				items: [
					{
						grant_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
						user_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
						endpoint_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
						enabled: true,
						quota_limit_bytes: 10737418240,
						cycle_policy: "inherit_user",
						cycle_day_of_month: null,
						note: null,
						credentials: {
							vless: {
								uuid: "00000000-0000-0000-0000-000000000000",
								email: "grant:01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
							},
						},
					},
				],
			}),
		).toEqual({
			items: [
				{
					grant_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
					user_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
					endpoint_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
					enabled: true,
					quota_limit_bytes: 10737418240,
					cycle_policy: "inherit_user",
					cycle_day_of_month: null,
					note: null,
					credentials: {
						vless: {
							uuid: "00000000-0000-0000-0000-000000000000",
							email: "grant:01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
						},
					},
				},
			],
		});
	});
});

describe("AdminGrantUsageResponseSchema", () => {
	it("accepts expected shape", () => {
		expect(
			AdminGrantUsageResponseSchema.parse({
				grant_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
				cycle_start_at: "2025-12-01T00:00:00+08:00",
				cycle_end_at: "2026-01-01T00:00:00+08:00",
				used_bytes: 123456789,
				owner_node_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
				desired_enabled: true,
				quota_banned: false,
				quota_banned_at: null,
				effective_enabled: true,
				warning: null,
			}),
		).toEqual({
			grant_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
			cycle_start_at: "2025-12-01T00:00:00+08:00",
			cycle_end_at: "2026-01-01T00:00:00+08:00",
			used_bytes: 123456789,
			owner_node_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
			desired_enabled: true,
			quota_banned: false,
			quota_banned_at: null,
			effective_enabled: true,
			warning: null,
		});
	});
});

describe("AdminJoinTokenResponseSchema", () => {
	it("accepts expected shape", () => {
		expect(
			AdminJoinTokenResponseSchema.parse({
				join_token: "base64url",
			}),
		).toEqual({
			join_token: "base64url",
		});
	});
});
