import { describe, expect, it } from "vitest";

import {
	realityServerNameSuggestionFromAccessHost,
	realityServerNameSuggestionFromDest,
	validateRealityServerName,
} from "./realityServerName";

describe("validateRealityServerName", () => {
	it("accepts typical hostnames used as SNI", () => {
		expect(validateRealityServerName("cdn-a.example.test")).toBeNull();
		expect(validateRealityServerName("download.example.com")).toBeNull();
		expect(validateRealityServerName("download.example.com:443")).toBeNull();
		expect(validateRealityServerName("  cdn-a.example.test  ")).toBeNull();
	});

	it("rejects common copy/paste mistakes (url/path)", () => {
		expect(
			validateRealityServerName("https://cdn-a.example.test"),
		).not.toBeNull();
		expect(validateRealityServerName("cdn-a.example.test/path")).not.toBeNull();
		expect(validateRealityServerName("cdn-a.example.test:0")).not.toBeNull();
		expect(
			validateRealityServerName("cdn-a.example.test:99999"),
		).not.toBeNull();
	});

	it("rejects invalid hostname formats", () => {
		expect(validateRealityServerName("")).not.toBeNull();
		expect(validateRealityServerName("cc.c")).not.toBeNull();
		expect(validateRealityServerName("localhost")).not.toBeNull();
		expect(validateRealityServerName("a..b.com")).not.toBeNull();
		expect(validateRealityServerName(".example.com")).not.toBeNull();
		expect(validateRealityServerName("example.com.")).not.toBeNull();
		expect(validateRealityServerName("ex_ample.com")).not.toBeNull();
		expect(validateRealityServerName("-example.com")).not.toBeNull();
		expect(validateRealityServerName("example-.com")).not.toBeNull();
	});
});

describe("realityServerNameSuggestionFromDest", () => {
	it("offers a valid destination authority as a serverName candidate", () => {
		expect(
			realityServerNameSuggestionFromDest(" origin.example.test:443 "),
		).toBe("origin.example.test:443");
	});

	it("does not turn urls or invalid destinations into candidates", () => {
		expect(
			realityServerNameSuggestionFromDest("https://origin.example.test:443"),
		).toBeNull();
		expect(realityServerNameSuggestionFromDest("localhost:443")).toBeNull();
	});
});

describe("realityServerNameSuggestionFromAccessHost", () => {
	it("offers the selected node access authority with the endpoint port", () => {
		expect(
			realityServerNameSuggestionFromAccessHost("node-xp.example.test", 443),
		).toBe("node-xp.example.test:443");
	});

	it("keeps explicit ports and rejects invalid access hosts", () => {
		expect(
			realityServerNameSuggestionFromAccessHost(
				"node-xp.example.test:8443",
				443,
			),
		).toBe("node-xp.example.test:8443");
		expect(
			realityServerNameSuggestionFromAccessHost(
				"https://node-xp.example.test",
				443,
			),
		).toBeNull();
		expect(
			realityServerNameSuggestionFromAccessHost("localhost", 443),
		).toBeNull();
	});
});
