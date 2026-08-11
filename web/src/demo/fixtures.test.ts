import { describe, expect, it } from "vitest";

import { fixtureCatalog } from "../fixture-policy/catalog";
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

	it("keeps the large scenario long-name node transformation", () => {
		const normalState = createDemoState("normal");
		const largeState = createDemoState("large");
		const tertiaryNodeId = fixtureCatalog.identifier.nodeTertiary();
		const normalTertiaryNode = normalState.nodes.find(
			(node) => node.id === tertiaryNodeId,
		);
		const largeTertiaryNode = largeState.nodes.find(
			(node) => node.id === tertiaryNodeId,
		);

		expect(largeTertiaryNode?.name).not.toBe(normalTertiaryNode?.name);
		expect(largeTertiaryNode?.name.length).toBeGreaterThan(32);
	});
});
