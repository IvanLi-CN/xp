import { describe, expect, it } from "vitest";

import {
	acceptedAuthoritySuggestionsFromAccessHost,
	canaryUpstreamSuggestionsFromManagedEndpoints,
	canaryUpstreamSuggestionsFromUrls,
} from "./managedVlessForm";

describe("canaryUpstreamSuggestionsFromUrls", () => {
	it("returns deduplicated normalized origin suggestions", () => {
		expect(
			canaryUpstreamSuggestionsFromUrls([
				"https://node.example.com:443/api/admin",
				"https://node.example.com",
				"http://127.0.0.1:8080/path",
			]),
		).toEqual([
			{
				value: "https://node.example.com",
				label: "https://node.example.com",
			},
			{
				value: "http://127.0.0.1:8080",
				label: "http://127.0.0.1:8080",
			},
		]);
	});

	it("rejects invalid or unsupported values", () => {
		expect(canaryUpstreamSuggestionsFromUrls([""])).toEqual([]);
		expect(
			canaryUpstreamSuggestionsFromUrls(["node.example.com:62416"]),
		).toEqual([]);
		expect(
			canaryUpstreamSuggestionsFromUrls(["ftp://node.example.com"]),
		).toEqual([]);
	});
});

describe("canaryUpstreamSuggestionsFromManagedEndpoints", () => {
	it("uses managed canary upstream origins from the selected node only", () => {
		expect(
			canaryUpstreamSuggestionsFromManagedEndpoints(
				[
					{
						endpoint_id: "managed-a",
						node_id: "node-alpha",
						kind: "vless_reality_vision_tcp",
						meta: {
							managed_default: true,
							canary_upstream: {
								url: "http://127.0.0.1:8080/path",
							},
						},
					},
					{
						endpoint_id: "managed-b",
						node_id: "node-alpha",
						kind: "vless_reality_vision_tcp",
						meta: {
							managed_default: true,
							canary_upstream: {
								url: "https://origin.example.com",
							},
						},
					},
					{
						endpoint_id: "managed-c",
						node_id: "node-beta",
						kind: "vless_reality_vision_tcp",
						meta: {
							managed_default: true,
							canary_upstream: {
								url: "https://other.example.com",
							},
						},
					},
					{
						endpoint_id: "legacy-a",
						node_id: "node-alpha",
						kind: "vless_reality_vision_tcp",
						meta: {
							managed_default: false,
							canary_upstream: {
								url: "https://legacy.example.com",
							},
						},
					},
				],
				{ nodeId: "node-alpha" },
			),
		).toEqual([
			{
				value: "http://127.0.0.1:8080",
				label: "http://127.0.0.1:8080",
			},
			{
				value: "https://origin.example.com",
				label: "https://origin.example.com",
			},
		]);
	});

	it("filters node-owned public and control-plane origins from suggestions", () => {
		expect(
			canaryUpstreamSuggestionsFromManagedEndpoints(
				[
					{
						endpoint_id: "managed-a",
						node_id: "node-alpha",
						kind: "vless_reality_vision_tcp",
						meta: {
							managed_default: true,
							canary_upstream: {
								url: "https://node-xp.example.test",
							},
						},
					},
					{
						endpoint_id: "managed-b",
						node_id: "node-alpha",
						kind: "vless_reality_vision_tcp",
						meta: {
							managed_default: true,
							canary_upstream: {
								url: "http://127.0.0.1:8080",
							},
						},
					},
				],
				{
					nodeId: "node-alpha",
					accessHost: "node-xp.example.test",
					apiBaseUrl: "https://node-xp.example.test:443",
				},
			),
		).toEqual([
			{
				value: "http://127.0.0.1:8080",
				label: "http://127.0.0.1:8080",
			},
		]);
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
