import { describe, expect, it } from "vitest";

import {
	acceptedAuthoritySuggestionsFromAccessHost,
	canaryUpstreamSuggestionsFromAuthorities,
	canaryUpstreamSuggestionsFromManagedEndpointDests,
} from "./managedVlessForm";

describe("canaryUpstreamSuggestionsFromAuthorities", () => {
	it("returns normalized XP HTTPS listener origin suggestions", () => {
		expect(
			canaryUpstreamSuggestionsFromAuthorities(["127.0.0.1:39043"]),
		).toEqual([
			{
				value: "https://127.0.0.1:39043",
				label: "https://127.0.0.1:39043",
			},
		]);
		expect(
			canaryUpstreamSuggestionsFromAuthorities([" Edge.Example.com. "]),
		).toEqual([
			{
				value: "https://edge.example.com",
				label: "https://edge.example.com",
			},
		]);
	});

	it("rejects invalid or unsupported values", () => {
		expect(canaryUpstreamSuggestionsFromAuthorities([""])).toEqual([]);
		expect(
			canaryUpstreamSuggestionsFromAuthorities(["https://node.example.com"]),
		).toEqual([]);
		expect(
			canaryUpstreamSuggestionsFromAuthorities(["node example.com:39043"]),
		).toEqual([]);
	});
});

describe("canaryUpstreamSuggestionsFromManagedEndpointDests", () => {
	it("uses same-node managed endpoint dest values as HTTPS listener suggestions", () => {
		expect(
			canaryUpstreamSuggestionsFromManagedEndpointDests(
				[
					{
						endpoint_id: "ep-managed",
						node_id: "node-a",
						kind: "vless_reality_vision_tcp",
						meta: {
							managed_default: true,
							reality: {
								dest: "127.0.0.1:39043",
							},
						},
					},
					{
						endpoint_id: "ep-manual",
						node_id: "node-a",
						kind: "vless_reality_vision_tcp",
						meta: {
							managed_default: false,
							reality: {
								dest: "origin.example.com:443",
							},
						},
					},
					{
						endpoint_id: "ep-other-node",
						node_id: "node-b",
						kind: "vless_reality_vision_tcp",
						meta: {
							managed_default: true,
							reality: {
								dest: "127.0.0.1:49043",
							},
						},
					},
				],
				"node-a",
			),
		).toEqual([
			{
				value: "https://127.0.0.1:39043",
				label: "https://127.0.0.1:39043",
			},
		]);
	});
});

describe("acceptedAuthoritySuggestionsFromAccessHost", () => {
	it("returns a normalized host suggestion that follows the endpoint port", () => {
		expect(
			acceptedAuthoritySuggestionsFromAccessHost(" Edge.Example.com. ", 443),
		).toEqual(["edge.example.com"]);
		expect(
			acceptedAuthoritySuggestionsFromAccessHost(" Edge.Example.com. ", 8443),
		).toEqual(["edge.example.com:8443"]);
		expect(
			acceptedAuthoritySuggestionsFromAccessHost("[2001:DB8::1]", 8443),
		).toEqual(["[2001:db8::1]:8443"]);
	});

	it("rejects invalid host or port values", () => {
		expect(acceptedAuthoritySuggestionsFromAccessHost("", 443)).toEqual([]);
		expect(
			acceptedAuthoritySuggestionsFromAccessHost(
				"https://edge.example.com",
				443,
			),
		).toEqual([]);
		expect(
			acceptedAuthoritySuggestionsFromAccessHost("edge.example.com", 0),
		).toEqual([]);
		expect(
			acceptedAuthoritySuggestionsFromAccessHost("edge.example.com", "abc"),
		).toEqual([]);
	});
});
