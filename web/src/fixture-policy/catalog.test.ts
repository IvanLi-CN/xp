import { describe, expect, it } from "vitest";

import { fixtureCatalog } from "@/fixture-policy/catalog";

describe("fixtureCatalog subscription token factory", () => {
	it("does not reuse an active token before the catalog sequence is exhausted", () => {
		const nextSubscriptionToken =
			fixtureCatalog.identifier.createSubscriptionTokenFactory();
		const tokens = Array.from({ length: 64 }, nextSubscriptionToken);

		expect(new Set(tokens).size).toBe(tokens.length);
		expect(nextSubscriptionToken).toThrow(
			"synthetic subscription token catalog exhausted",
		);
	});

	it("keeps endpoint and user identifier sequences scoped to each factory", () => {
		const firstEndpointFactory =
			fixtureCatalog.identifier.createEndpointIdFactory();
		const secondEndpointFactory =
			fixtureCatalog.identifier.createEndpointIdFactory();
		expect(secondEndpointFactory()).toBe(firstEndpointFactory());

		const firstUserFactory = fixtureCatalog.identifier.createUserIdFactory();
		const secondUserFactory = fixtureCatalog.identifier.createUserIdFactory();
		expect([
			firstUserFactory(),
			firstUserFactory(),
			firstUserFactory(),
		]).toEqual([
			fixtureCatalog.identifier.userTertiary(),
			fixtureCatalog.identifier.userQuaternary(),
			fixtureCatalog.identifier.userQuinary(),
		]);
		expect(secondUserFactory()).toBe(fixtureCatalog.identifier.userTertiary());
	});

	it("exposes named fixture values without positional slot access", () => {
		expect(fixtureCatalog.nodeId.fixture229()).toBe("node-fixture-229");
		expect(fixtureCatalog.nodeName.fixture18()).toBe("fixture-node-18");
		expect(fixtureCatalog.host.fixture231()).toBe("host-231.fixture.test");
		expect(fixtureCatalog.service.fixture230()).toBe(
			"https://service-230.fixture.test",
		);
		expect("slotString" in fixtureCatalog).toBe(false);
	});

	it("uses page-role names for the NodesPage story fixture", () => {
		expect(fixtureCatalog.story.nodesPagePrimaryNodeId()).toBe(
			"node-fixture-229",
		);
		expect(fixtureCatalog.story.nodesPagePrimaryNodeName()).toBe(
			"node-fixture-106",
		);
	});
});
