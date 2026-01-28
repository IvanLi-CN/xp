import { describe, expect, it } from "vitest";

import {
	AdminEndpointRotateResponseSchema,
	AdminEndpointSchema,
	AdminEndpointsResponseSchema,
} from "./adminEndpoints";
import {
	AdminGrantGroupDetailSchema,
	AdminGrantGroupsResponseSchema,
} from "./adminGrantGroups";
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
				xp_version: "0.0.0",
			}),
		).toEqual({
			cluster_id: "01HZXKQF2Z6C8W8E9Y5C8M0X8Q",
			node_id: "01HZXKQF2Z6C8W8E9Y5C8M0X8Q",
			role: "leader",
			leader_api_base_url: "https://127.0.0.1:62416",
			term: 1,
			xp_version: "0.0.0",
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
						access_host: "",
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: null,
						},
					},
				],
			}),
		).toEqual({
			items: [
				{
					node_id: "01HZXKQF2Z6C8W8E9Y5C8M0X8Q",
					node_name: "node-1",
					api_base_url: "https://127.0.0.1:62416",
					access_host: "",
					quota_reset: {
						policy: "monthly",
						day_of_month: 1,
						tz_offset_minutes: null,
					},
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
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: 480,
						},
					},
				],
			}),
		).toEqual({
			items: [
				{
					user_id: "01JZXKQF2Z6C8W8E9Y5C8M0X8Q",
					display_name: "alice",
					subscription_token: "sub_123",
					quota_reset: {
						policy: "monthly",
						day_of_month: 1,
						tz_offset_minutes: 480,
					},
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

describe("AdminGrantGroupsResponseSchema", () => {
	it("accepts expected shape", () => {
		expect(
			AdminGrantGroupsResponseSchema.parse({
				items: [
					{
						group_name: "group-1",
						member_count: 2,
					},
				],
			}),
		).toEqual({
			items: [
				{
					group_name: "group-1",
					member_count: 2,
				},
			],
		});
	});
});

describe("AdminGrantGroupDetailSchema", () => {
	it("accepts expected shape", () => {
		expect(
			AdminGrantGroupDetailSchema.parse({
				group: { group_name: "group-1" },
				members: [
					{
						user_id: "user-1",
						endpoint_id: "endpoint-1",
						enabled: true,
						quota_limit_bytes: 10737418240,
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
			group: { group_name: "group-1" },
			members: [
				{
					user_id: "user-1",
					endpoint_id: "endpoint-1",
					enabled: true,
					quota_limit_bytes: 10737418240,
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
