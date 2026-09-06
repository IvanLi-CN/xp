import { describe, expect, it } from "vitest";

import { applyRuntimePolicyToCsp } from "./runtimePolicyCsp";

describe("applyRuntimePolicyToCsp", () => {
	it("replaces only connect-src and keeps other directives", () => {
		expect(
			applyRuntimePolicyToCsp(
				"default-src 'self'; connect-src 'self' https://old.example; img-src 'self'",
				[
					"https://node-b.example",
					"https://node-a.example",
					"https://node-a.example",
				],
			),
		).toBe(
			[
				"default-src 'self'; connect-src 'self' ",
				"https://node-a.example https://node-b.example; img-src 'self'",
			].join(""),
		);
	});

	it("adds connect-src when the source policy omitted it", () => {
		expect(
			applyRuntimePolicyToCsp("default-src 'self'", ["https://node.example"]),
		).toBe("default-src 'self'; connect-src 'self' https://node.example");
	});
});
