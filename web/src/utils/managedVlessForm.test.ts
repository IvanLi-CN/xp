import { describe, expect, it } from "vitest";

import {
	acceptedAuthoritySuggestionsFromAccessHost,
	canaryUpstreamSuggestionsFromApiBaseUrl,
} from "./managedVlessForm";

describe("canaryUpstreamSuggestionsFromApiBaseUrl", () => {
	it("returns a normalized origin suggestion", () => {
		expect(
			canaryUpstreamSuggestionsFromApiBaseUrl(
				"https://node.example.com:443/api/admin",
			),
		).toEqual([
			{
				value: "https://node.example.com",
				label: "https://node.example.com",
			},
		]);
	});

	it("rejects invalid or unsupported values", () => {
		expect(canaryUpstreamSuggestionsFromApiBaseUrl("")).toEqual([]);
		expect(
			canaryUpstreamSuggestionsFromApiBaseUrl("node.example.com:62416"),
		).toEqual([]);
		expect(
			canaryUpstreamSuggestionsFromApiBaseUrl("ftp://node.example.com"),
		).toEqual([]);
	});
});

describe("acceptedAuthoritySuggestionsFromAccessHost", () => {
	it("returns a normalized host suggestion without the default port", () => {
		expect(
			acceptedAuthoritySuggestionsFromAccessHost(" Edge.Example.com. "),
		).toEqual(["edge.example.com"]);
		expect(acceptedAuthoritySuggestionsFromAccessHost("[2001:DB8::1]")).toEqual(
			["[2001:db8::1]"],
		);
	});

	it("rejects invalid host values", () => {
		expect(acceptedAuthoritySuggestionsFromAccessHost("")).toEqual([]);
		expect(
			acceptedAuthoritySuggestionsFromAccessHost("https://edge.example.com"),
		).toEqual([]);
	});
});
