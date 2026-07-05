import { describe, expect, it } from "vitest";

import {
	realityDestFromApiBaseUrl,
	realityDestFromBindAddress,
} from "./realityDestSuggestions";

describe("realityDestFromApiBaseUrl", () => {
	it("keeps explicit api ports", () => {
		expect(realityDestFromApiBaseUrl("https://node.example.com:62416")).toBe(
			"node.example.com:62416",
		);
	});

	it("adds default ports for http and https api URLs", () => {
		expect(realityDestFromApiBaseUrl("https://node.example.com")).toBe(
			"node.example.com:443",
		);
		expect(realityDestFromApiBaseUrl("http://node.example.com")).toBe(
			"node.example.com:80",
		);
	});

	it("ignores URL paths and invalid values", () => {
		expect(
			realityDestFromApiBaseUrl("https://node.example.com/api/admin"),
		).toBe("node.example.com:443");
		expect(realityDestFromApiBaseUrl("node.example.com:62416")).toBeNull();
		expect(realityDestFromApiBaseUrl("")).toBeNull();
	});
});

describe("realityDestFromBindAddress", () => {
	it("uses the local xp listener host and port", () => {
		expect(realityDestFromBindAddress("127.0.0.1:62416")).toBe(
			"127.0.0.1:62416",
		);
		expect(realityDestFromBindAddress("0.0.0.0:62416")).toBe("0.0.0.0:62416");
		expect(realityDestFromBindAddress("[::1]:62416")).toBe("[::1]:62416");
	});

	it("rejects incomplete bind addresses", () => {
		expect(realityDestFromBindAddress("127.0.0.1")).toBeNull();
		expect(realityDestFromBindAddress("::1:62416")).toBeNull();
		expect(realityDestFromBindAddress("127.0.0.1:")).toBeNull();
		expect(realityDestFromBindAddress("")).toBeNull();
	});
});
