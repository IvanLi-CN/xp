import { describe, expect, it } from "vitest";
import { fixtureCatalog } from "../fixture-policy/catalog";

import {
	AdminEndpointRotateResponseSchema,
	AdminEndpointSchema,
	AdminEndpointsResponseSchema,
} from "./adminEndpoints";
import { AdminJoinTokenResponseSchema } from "./adminJoinTokens";
import { AdminNodesResponseSchema } from "./adminNodes";
import { AdminQuotaPolicyNodeWeightRowsResponseSchema } from "./adminQuotaPolicyNodeWeightRows";
import {
	GetAdminUserAccessResponseSchema,
	PutAdminUserAccessResponseSchema,
} from "./adminUserAccess";
import {
	AdminUserMihomoProfileSchema,
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
				cluster_id: fixtureCatalog.cluster.fixture95(),
				node_id: fixtureCatalog.cluster.fixture95(),
				role: "leader",
				leader_api_base_url: fixtureCatalog.service.fixture96(),
				term: 1,
				xp_version: "0.0.0",
			}),
		).toEqual({
			cluster_id: fixtureCatalog.cluster.fixture95(),
			node_id: fixtureCatalog.cluster.fixture95(),
			role: "leader",
			leader_api_base_url: fixtureCatalog.service.fixture96(),
			term: 1,
			xp_version: "0.0.0",
		});
	});

	it("rejects missing fields declared by every pinned release", () => {
		expect(() =>
			ClusterInfoResponseSchema.parse({
				cluster_id: fixtureCatalog.cluster.fixture97(),
				node_id: fixtureCatalog.nodeId.fixture98(),
				role: "leader",
			}),
		).toThrow();
	});
});

describe("AdminNodesResponseSchema", () => {
	it("accepts expected shape", () => {
		expect(
			AdminNodesResponseSchema.parse({
				items: [
					{
						node_id: fixtureCatalog.cluster.fixture95(),
						node_name: fixtureCatalog.nodeId.fixture32(),
						api_base_url: fixtureCatalog.service.fixture96(),
						access_host: fixtureCatalog.host.fixture99(),
						quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
						quota_reset: fixtureCatalog.quota.reset(),
					},
				],
			}),
		).toEqual({
			items: [
				{
					node_id: fixtureCatalog.cluster.fixture95(),
					node_name: fixtureCatalog.nodeId.fixture32(),
					api_base_url: fixtureCatalog.service.fixture96(),
					access_host: fixtureCatalog.host.fixture99(),
					quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
					quota_reset: fixtureCatalog.quota.reset(),
				},
			],
		});
	});
});

describe("AdminEndpointSchema", () => {
	it("accepts expected shape", () => {
		expect(
			AdminEndpointSchema.parse({
				endpoint_id: fixtureCatalog.endpointId.fixture100(),
				node_id: fixtureCatalog.endpointId.fixture100(),
				tag: fixtureCatalog.endpointTag.fixture101(),
				kind: fixtureCatalog.endpoint.vlessKind(),
				port: fixtureCatalog.endpoint.port443(),
				meta: {},
			}),
		).toEqual({
			endpoint_id: fixtureCatalog.endpointId.fixture100(),
			node_id: fixtureCatalog.endpointId.fixture100(),
			tag: fixtureCatalog.endpointTag.fixture101(),
			kind: fixtureCatalog.endpoint.vlessKind(),
			port: fixtureCatalog.endpoint.port443(),
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
						endpoint_id: fixtureCatalog.endpointId.fixture100(),
						node_id: fixtureCatalog.endpointId.fixture100(),
						tag: fixtureCatalog.endpointTag.fixture101(),
						kind: fixtureCatalog.endpoint.vlessKind(),
						port: fixtureCatalog.endpoint.port443(),
						meta: {},
					},
				],
			}),
		).toEqual({
			items: [
				{
					endpoint_id: fixtureCatalog.endpointId.fixture100(),
					node_id: fixtureCatalog.endpointId.fixture100(),
					tag: fixtureCatalog.endpointTag.fixture101(),
					kind: fixtureCatalog.endpoint.vlessKind(),
					port: fixtureCatalog.endpoint.port443(),
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
				endpoint_id: fixtureCatalog.endpointId.fixture100(),
				active_short_id: "0123456789abcdef",
				short_ids: ["0123456789abcdef", "0123456789abcdff"],
			}),
		).toEqual({
			endpoint_id: fixtureCatalog.endpointId.fixture100(),
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
						user_id: fixtureCatalog.endpointId.fixture100(),
						display_name: "alice",
						subscription_token: fixtureCatalog.token.fixture102(),
						credential_epoch: 0,
						priority_tier: "p3",
						quota_reset: fixtureCatalog.quota.reset(),
					},
				],
			}),
		).toEqual({
			items: [
				{
					user_id: fixtureCatalog.endpointId.fixture100(),
					display_name: "alice",
					subscription_token: fixtureCatalog.token.fixture102(),
					credential_epoch: 0,
					priority_tier: "p3",
					quota_reset: fixtureCatalog.quota.reset(),
				},
			],
		});
	});
});

describe("AdminQuotaPolicyNodeWeightRowsResponseSchema", () => {
	it("accepts expected shape", () => {
		expect(
			AdminQuotaPolicyNodeWeightRowsResponseSchema.parse({
				items: [
					{
						user_id: fixtureCatalog.identifier.userPrimary(),
						display_name: "alice",
						priority_tier: "p1",
						endpoint_ids: [fixtureCatalog.endpointId.fixture40()],
						stored_weight: 6000,
						editor_weight: 6000,
						source: "explicit",
					},
					{
						user_id: fixtureCatalog.identifier.userSecondary(),
						display_name: "bob",
						priority_tier: "p2",
						endpoint_ids: ["endpoint-2"],
						editor_weight: 0,
						source: "implicit_zero",
					},
				],
			}),
		).toEqual({
			items: [
				{
					user_id: fixtureCatalog.identifier.userPrimary(),
					display_name: "alice",
					priority_tier: "p1",
					endpoint_ids: [fixtureCatalog.endpointId.fixture40()],
					stored_weight: 6000,
					editor_weight: 6000,
					source: "explicit",
				},
				{
					user_id: fixtureCatalog.identifier.userSecondary(),
					display_name: "bob",
					priority_tier: "p2",
					endpoint_ids: ["endpoint-2"],
					editor_weight: 0,
					source: "implicit_zero",
				},
			],
		});
	});
});

describe("AdminUserTokenResponseSchema", () => {
	it("accepts expected shape", () => {
		expect(
			AdminUserTokenResponseSchema.parse({
				subscription_token: fixtureCatalog.token.fixture102(),
			}),
		).toEqual({
			subscription_token: fixtureCatalog.token.fixture102(),
		});
	});
});

describe("AdminUserMihomoProfileSchema", () => {
	it("accepts the mixin field from current responses", () => {
		expect(
			AdminUserMihomoProfileSchema.parse({
				mixin_yaml: "port: 0\n",
				extra_proxies_yaml: fixtureCatalog.string.none(),
				extra_proxy_providers_yaml: fixtureCatalog.string.none(),
			}),
		).toEqual({
			mixin_yaml: "port: 0\n",
			extra_proxies_yaml: fixtureCatalog.string.none(),
			extra_proxy_providers_yaml: fixtureCatalog.string.none(),
		});
	});

	it("rejects legacy template_yaml-only responses", () => {
		expect(() =>
			AdminUserMihomoProfileSchema.parse({
				template_yaml: "port: 7890\n",
				extra_proxies_yaml: fixtureCatalog.string.none(),
				extra_proxy_providers_yaml: fixtureCatalog.string.none(),
			}),
		).toThrow();
	});
});

describe("GetAdminUserAccessResponseSchema", () => {
	it("accepts expected shape", () => {
		expect(
			GetAdminUserAccessResponseSchema.parse({
				items: [
					{
						user_id: fixtureCatalog.identifier.userPrimary(),
						endpoint_id: fixtureCatalog.endpointId.fixture40(),
						node_id: fixtureCatalog.nodeId.fixture32(),
					},
				],
				auto_assign_endpoint_kinds: ["vless_reality_vision_tcp"],
			}),
		).toEqual({
			items: [
				{
					user_id: fixtureCatalog.identifier.userPrimary(),
					endpoint_id: fixtureCatalog.endpointId.fixture40(),
					node_id: fixtureCatalog.nodeId.fixture32(),
				},
			],
			auto_assign_endpoint_kinds: ["vless_reality_vision_tcp"],
		});
	});
});

describe("PutAdminUserAccessResponseSchema", () => {
	it("accepts expected shape", () => {
		expect(
			PutAdminUserAccessResponseSchema.parse({
				created: 1,
				deleted: 2,
				items: [
					{
						user_id: fixtureCatalog.identifier.userPrimary(),
						endpoint_id: fixtureCatalog.endpointId.fixture40(),
						node_id: fixtureCatalog.nodeId.fixture32(),
					},
				],
				auto_assign_endpoint_kinds: ["ss2022_2022_blake3_aes_128_gcm"],
			}),
		).toEqual({
			created: 1,
			deleted: 2,
			items: [
				{
					user_id: fixtureCatalog.identifier.userPrimary(),
					endpoint_id: fixtureCatalog.endpointId.fixture40(),
					node_id: fixtureCatalog.nodeId.fixture32(),
				},
			],
			auto_assign_endpoint_kinds: ["ss2022_2022_blake3_aes_128_gcm"],
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
