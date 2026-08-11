import { describe, expect, it } from "vitest";
import { fixtureCatalog } from "../fixture-policy/catalog";

import {
	acceptedAuthoritySuggestionsFromAccessHost,
	canaryUpstreamSuggestionsFromAuthorities,
	canaryUpstreamSuggestionsFromManagedEndpointDests,
	mergeManagedVlessAutocompleteSuggestions,
} from "./managedVlessForm";

describe("canaryUpstreamSuggestionsFromAuthorities", () => {
	it("returns normalized XP HTTPS listener origin suggestions", () => {
		expect(
			canaryUpstreamSuggestionsFromAuthorities([
				fixtureCatalog.address.loopback39043(),
			]),
		).toEqual([
			{
				value: fixtureCatalog.canaryUpstream.httpsListener().url,
				label: fixtureCatalog.canaryUpstream.httpsListener().url,
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
						endpoint_id: fixtureCatalog.endpointId.fixture105(),
						node_id: fixtureCatalog.nodeId.fixture106(),
						kind: fixtureCatalog.endpoint.vlessKind(),
						meta: {
							managed_default: true,
							reality: {
								dest: fixtureCatalog.address.loopback39043(),
							},
						},
					},
					{
						endpoint_id: fixtureCatalog.endpointId.fixture107(),
						node_id: fixtureCatalog.nodeId.fixture106(),
						kind: fixtureCatalog.endpoint.vlessKind(),
						meta: {
							managed_default: false,
							reality: {
								dest: fixtureCatalog.address.loopbackPort39108(),
							},
						},
					},
					{
						endpoint_id: fixtureCatalog.endpointId.fixture109(),
						node_id: fixtureCatalog.nodeId.fixture110(),
						kind: fixtureCatalog.endpoint.vlessKind(),
						meta: {
							managed_default: true,
							reality: {
								dest: fixtureCatalog.address.loopback49043(),
							},
						},
					},
				],
				fixtureCatalog.nodeId.fixture106(),
			),
		).toEqual([
			{
				value: fixtureCatalog.canaryUpstream.httpsListener().url,
				label: fixtureCatalog.canaryUpstream.httpsListener().url,
			},
		]);
	});
});

describe("mergeManagedVlessAutocompleteSuggestions", () => {
	it("preserves existing origin suggestions before fallback authorities", () => {
		expect(
			mergeManagedVlessAutocompleteSuggestions([
				{
					value: fixtureCatalog.canaryUpstream.httpsAlternate().url,
					label: fixtureCatalog.canaryUpstream.httpsAlternate().url,
				},
				...canaryUpstreamSuggestionsFromAuthorities([
					fixtureCatalog.address.loopback39043(),
					fixtureCatalog.address.loopback49043(),
				]),
			]),
		).toEqual([
			{
				value: fixtureCatalog.canaryUpstream.httpsAlternate().url,
				label: fixtureCatalog.canaryUpstream.httpsAlternate().url,
			},
			{
				value: fixtureCatalog.canaryUpstream.httpsListener().url,
				label: fixtureCatalog.canaryUpstream.httpsListener().url,
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
