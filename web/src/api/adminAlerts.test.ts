import { describe, expect, it } from "vitest";

import { AlertsResponseSchema } from "./adminAlerts";

describe("admin alert schemas", () => {
	it("requires the server completeness fields", () => {
		expect(() => AlertsResponseSchema.parse({ items: [] })).toThrow();
	});
});
