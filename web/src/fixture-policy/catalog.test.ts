import { describe, expect, it } from "vitest";

import { fixtureCatalog } from "@/fixture-policy/catalog";

describe("fixtureCatalog subscription token factory", () => {
	it("does not reuse an active token after the catalog sequence is exhausted", () => {
		const nextSubscriptionToken =
			fixtureCatalog.identifier.createSubscriptionTokenFactory();
		const tokens = [
			nextSubscriptionToken(),
			nextSubscriptionToken(),
			nextSubscriptionToken(),
			nextSubscriptionToken(),
			nextSubscriptionToken(),
		];

		expect(new Set(tokens).size).toBe(tokens.length);
		expect(nextSubscriptionToken).toThrow(
			"synthetic subscription token sequence exhausted",
		);
	});
});
