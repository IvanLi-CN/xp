import { describe, expect, it } from "vitest";

import { DEMO_SCENARIOS, createDemoState } from "./fixtures";

describe("demo fixtures", () => {
	it("identifies a node in every scenario as the local web host", () => {
		for (const scenario of DEMO_SCENARIOS) {
			const state = createDemoState(scenario.id);
			expect(state.nodes.some((node) => node.id === state.localNodeId)).toBe(
				true,
			);
		}
	});
});
