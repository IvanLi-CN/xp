import { describe, expect, it } from "vitest";

import type { AdminEndpointsResponse } from "../api/adminEndpoints";
import {
	appendAdminEndpoint,
	removeAdminEndpoints,
	replaceAdminEndpoint,
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
});
