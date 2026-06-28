import { describe, expect, it } from "vitest";

import {
	normalizeAcceptedAuthority,
	validateAcceptedAuthority,
} from "./acceptedAuthority";

describe("acceptedAuthority utils", () => {
	it("normalizes case and trailing dots for hostname authorities", () => {
		expect(normalizeAcceptedAuthority(" Edge.Example.com.:443 ")).toBe(
			"edge.example.com:443",
		);
		expect(normalizeAcceptedAuthority("[2001:DB8::1]:8443")).toBe(
			"[2001:db8::1]:8443",
		);
	});

	it("accepts valid hostname, IPv4, and bracketed IPv6 authorities", () => {
		expect(validateAcceptedAuthority("edge.example.com:443")).toBeNull();
		expect(validateAcceptedAuthority("203.0.113.10:8443")).toBeNull();
		expect(validateAcceptedAuthority("[2001:db8::1]:8443")).toBeNull();
	});

	it("rejects missing port, URL-like input, and bare IPv6", () => {
		expect(validateAcceptedAuthority("edge.example.com")).not.toBeNull();
		expect(
			validateAcceptedAuthority("https://edge.example.com:443"),
		).not.toBeNull();
		expect(validateAcceptedAuthority("2001:db8::1:8443")).not.toBeNull();
	});
});
