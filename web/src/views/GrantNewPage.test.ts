import { describe, expect, it } from "vitest";

import { buildGrantGroupCreateRequest } from "./GrantNewPage";

describe("buildGrantGroupCreateRequest", () => {
	it("builds a single group create payload for multiple selected endpoints", () => {
		const payload = buildGrantGroupCreateRequest({
			groupName: "  group-20260119-abcd  ",
			userId: "u_01HUSERAAAAAA",
			selectedEndpointIds: ["ep-a", "ep-b"],
			endpoints: [
				{
					endpoint_id: "ep-a",
					node_id: "n1",
					tag: "tokyo-vless",
					kind: "vless_reality_vision_tcp",
					port: 443,
					meta: {},
				},
				{
					endpoint_id: "ep-b",
					node_id: "n2",
					tag: "osaka-ss",
					kind: "ss2022_2022_blake3_aes_128_gcm",
					port: 8443,
					meta: {},
				},
			],
			nodeQuotas: [
				{
					user_id: "u_01HUSERAAAAAA",
					node_id: "n1",
					quota_limit_bytes: 10,
				},
			],
			note: "  enterprise quota  ",
		});

		expect(payload.group_name).toBe("group-20260119-abcd");
		expect(payload.members).toEqual([
			{
				user_id: "u_01HUSERAAAAAA",
				endpoint_id: "ep-a",
				enabled: true,
				quota_limit_bytes: 10,
				note: "enterprise quota",
			},
			{
				user_id: "u_01HUSERAAAAAA",
				endpoint_id: "ep-b",
				enabled: true,
				quota_limit_bytes: 0,
				note: "enterprise quota",
			},
		]);
	});

	it("sets note to null when empty", () => {
		const payload = buildGrantGroupCreateRequest({
			groupName: "group-1",
			userId: "u1",
			selectedEndpointIds: ["ep-a"],
			endpoints: [
				{
					endpoint_id: "ep-a",
					node_id: "n1",
					tag: "tokyo-vless",
					kind: "vless_reality_vision_tcp",
					port: 443,
					meta: {},
				},
			],
			nodeQuotas: [],
			note: "   ",
		});

		expect(payload.members[0]?.note).toBeNull();
	});

	it("throws when a selected endpoint is missing", () => {
		expect(() =>
			buildGrantGroupCreateRequest({
				groupName: "group-1",
				userId: "u1",
				selectedEndpointIds: ["missing"],
				endpoints: [],
				nodeQuotas: [],
				note: "",
			}),
		).toThrow(/endpoint not found/);
	});
});
