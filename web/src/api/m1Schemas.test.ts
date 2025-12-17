import { describe, expect, it } from "vitest";

import { AdminNodesResponseSchema } from "./adminNodes";
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
