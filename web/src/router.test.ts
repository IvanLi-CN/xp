import { describe, expect, it, vi } from "vitest";

import { createAppRouter } from "./router";

vi.mock("./components/PwaStatusPrompt", () => ({
	PwaStatusPrompt: () => null,
}));

describe("Nodes route matching", () => {
	it("prefers static module paths over the node ID route", () => {
		const router = createAppRouter();

		expect(router.matchRoutes("/nodes").at(-1)?.routeId).toBe("/app/nodes");
		expect(router.matchRoutes("/nodes/join").at(-1)?.routeId).toBe(
			"/app/nodes/join",
		);
		expect(router.matchRoutes("/nodes/repositories").at(-1)?.routeId).toBe(
			"/app/nodes/repositories",
		);
		expect(router.matchRoutes("/nodes/node-123").at(-1)?.routeId).toBe(
			"/app/nodes/$nodeId",
		);
	});
});
