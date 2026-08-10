import { describe, expect, it } from "vitest";

import { fixtureCatalog } from "../fixture-policy/catalog";
import { resolveUserTrafficNodeFilter } from "./userTrafficNodeFilter";

const options = [
	{
		node_id: fixtureCatalog.identifier.nodePrimary(),
		node_name: fixtureCatalog.identifier.nodeNamePrimary(),
	},
	{
		node_id: fixtureCatalog.identifier.nodeSecondary(),
		node_name: fixtureCatalog.identifier.nodeNameSecondary(),
	},
];

describe("resolveUserTrafficNodeFilter", () => {
	it("retains a filter only when it belongs to the current user's options", () => {
		expect(
			resolveUserTrafficNodeFilter({
				activeNodeId: fixtureCatalog.identifier.nodePrimary(),
				options,
				optionsUserId: fixtureCatalog.identifier.userPrimary(),
				userId: fixtureCatalog.identifier.userPrimary(),
			}),
		).toBe(fixtureCatalog.identifier.nodePrimary());
		expect(
			resolveUserTrafficNodeFilter({
				activeNodeId: fixtureCatalog.identifier.nodePrimary(),
				options: [options[1]],
				optionsUserId: fixtureCatalog.identifier.userSecondary(),
				userId: fixtureCatalog.identifier.userSecondary(),
			}),
		).toBeNull();
	});

	it("uses all nodes until the new user's options have loaded", () => {
		expect(
			resolveUserTrafficNodeFilter({
				activeNodeId: fixtureCatalog.identifier.nodePrimary(),
				options,
				optionsUserId: fixtureCatalog.identifier.userPrimary(),
				userId: fixtureCatalog.identifier.userSecondary(),
			}),
		).toBeNull();
	});
});
