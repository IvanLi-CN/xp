import { describe, expect, it } from "vitest";

import { resolveUserTrafficNodeFilter } from "./userTrafficNodeFilter";

const options = [
	{ node_id: "node-a", node_name: "Node A" },
	{ node_id: "node-b", node_name: "Node B" },
];

describe("resolveUserTrafficNodeFilter", () => {
	it("retains a filter only when it belongs to the current user's options", () => {
		expect(
			resolveUserTrafficNodeFilter({
				activeNodeId: "node-a",
				options,
				optionsUserId: "user-a",
				userId: "user-a",
			}),
		).toBe("node-a");
		expect(
			resolveUserTrafficNodeFilter({
				activeNodeId: "node-a",
				options: [{ node_id: "node-b", node_name: "Node B" }],
				optionsUserId: "user-b",
				userId: "user-b",
			}),
		).toBeNull();
	});

	it("uses all nodes until the new user's options have loaded", () => {
		expect(
			resolveUserTrafficNodeFilter({
				activeNodeId: "node-a",
				options,
				optionsUserId: "user-a",
				userId: "user-b",
			}),
		).toBeNull();
	});
});
