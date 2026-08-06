import { describe, expect, it } from "vitest";

import { HealthResponseSchema } from "./health";

describe("HealthResponseSchema", () => {
	it("accepts ok response", () => {
		expect(HealthResponseSchema.parse({ status: "ok" })).toEqual({
			status: "ok",
		});
	});

	it("keeps additive fields from a newer server", () => {
		expect(
			HealthResponseSchema.parse({ status: "ok", build_id: "server-build" }),
		).toEqual({ status: "ok", build_id: "server-build" });
	});

	it("rejects unexpected payloads", () => {
		expect(() => HealthResponseSchema.parse({ status: "nope" })).toThrow();
		expect(() => HealthResponseSchema.parse({})).toThrow();
	});
});
