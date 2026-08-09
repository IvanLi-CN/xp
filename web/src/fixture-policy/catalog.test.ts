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

	it("does not expose slots beyond the checked-in catalog", () => {
		// @ts-expect-error catalog has slots s0 through s684
		expect(fixtureCatalog.slotString.s685).toBeUndefined();
	});
});
