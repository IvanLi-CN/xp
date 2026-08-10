import { describe, expect, it } from "vitest";

import type { AdminEndpointsResponse } from "../api/adminEndpoints";
import { createQueryClient } from "../queryClient";
import {
	appendAdminEndpoint,
	removeAdminEndpoints,
	replaceAdminEndpoint,
	resourceListCache,
	syncNode,
} from "./adminEndpointsCache";

const endpoint = {
	endpoint_id: "endpoint-1",
	node_id: "node-1",
	tag: "primary",
	kind: "vless_reality_vision_tcp" as const,
	port: 443,
	meta: {},
};

const cached: AdminEndpointsResponse = {
	items: [endpoint, { ...endpoint, endpoint_id: "endpoint-2", tag: "backup" }],
};

describe("admin endpoint cache", () => {
	it("preserves API order while applying navigation list mutations", () => {
		expect(
			appendAdminEndpoint(cached, {
				...endpoint,
				endpoint_id: "endpoint-3",
			})?.items.map((item) => item.endpoint_id),
		).toEqual(["endpoint-1", "endpoint-2", "endpoint-3"]);
		expect(
			replaceAdminEndpoint(cached, { ...endpoint, tag: "renamed" })?.items[0]
				.tag,
		).toBe("renamed");
		expect(
			removeAdminEndpoints(cached, ["endpoint-1", "endpoint-2"])?.items,
		).toEqual([]);
	});

	it("synchronizes updates and removes deleted resource detail caches", () => {
		const queryClient = createQueryClient();
		queryClient.setQueryData(["adminNodes", "token"], {
			items: [
				{
					node_id: "node-1",
					node_name: "Tokyo",
					quota_limit_bytes: 0,
				},
			],
		});
		queryClient.setQueryData(
			["adminEndpoint", "token", "endpoint-1"],
			endpoint,
		);
		queryClient.setQueryData(["adminNode", "token", "node-1"], {
			node_id: "node-1",
		});

		syncNode(queryClient, "token", "node-1", { quota_limit_bytes: 1024 });
		expect(
			queryClient.getQueryData<{ items: Array<{ quota_limit_bytes: number }> }>(
				["adminNodes", "token"],
			)?.items[0].quota_limit_bytes,
		).toBe(1024);

		resourceListCache.remove(queryClient, "token", "endpoint-1");
		expect(
			queryClient.getQueryData(["adminEndpoint", "token", "endpoint-1"]),
		).toBeUndefined();
		queryClient.setQueryData(["adminEndpoint", "token", "endpoint-2"], {
			...endpoint,
			endpoint_id: "endpoint-2",
		});
		resourceListCache.nodeDeleted(queryClient, "token", "node-1");
		expect(
			queryClient.getQueryData(["adminNode", "token", "node-1"]),
		).toBeUndefined();
		expect(
			queryClient.getQueryData(["adminEndpoint", "token", "endpoint-2"]),
		).toBeUndefined();
	});
});
