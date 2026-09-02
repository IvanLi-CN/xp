import { describe, expect, it } from "vitest";

import { fixtureCatalog } from "../fixture-policy/catalog";
import {
	normalizeMihomoPrivateCidr,
	validateMihomoPrivateCidr,
} from "./mihomoPrivateCidr";

describe("validateMihomoPrivateCidr", () => {
	it("accepts RFC1918 IPv4 and IPv6 ULA CIDRs", () => {
		expect(
			validateMihomoPrivateCidr(fixtureCatalog.address.privateCidr()),
		).toBeNull();
		expect(
			validateMihomoPrivateCidr(fixtureCatalog.address.privateIpv6Cidr()),
		).toBeNull();
	});

	it("normalizes bare private IP literals to host CIDRs", () => {
		expect(normalizeMihomoPrivateCidr(" 192.168.31.11 ")).toBe(
			"192.168.31.11/32",
		);
		expect(normalizeMihomoPrivateCidr("fd12:3456::7")).toBe("fd12:3456::7/128");
		expect(validateMihomoPrivateCidr("192.168.31.11")).toBeNull();
		expect(validateMihomoPrivateCidr("fd12:3456::7")).toBeNull();
	});

	it("rejects malformed addresses and invalid prefix lengths", () => {
		expect(validateMihomoPrivateCidr("192.168.31")).toBe(
			"CIDR address must be a valid IPv4 or IPv6 literal.",
		);
		expect(
			validateMihomoPrivateCidr(
				fixtureCatalog.address.privateCidr().replace("/24", "/33"),
			),
		).toBe("IPv4 CIDR prefix must be between 0 and 32.");
		expect(
			validateMihomoPrivateCidr(
				fixtureCatalog.address.privateIpv6Cidr().replace("/64", "/129"),
			),
		).toBe("IPv6 CIDR prefix must be between 0 and 128.");
	});

	it("rejects CIDRs outside the server policy boundary", () => {
		const publicCidr = `${fixtureCatalog.address.primaryIpv4().replace(/\.\d+$/, ".0")}/24`;
		expect(validateMihomoPrivateCidr(publicCidr)).toBe(
			"Only RFC1918 IPv4 CIDRs or IPv6 ULA CIDRs (fc00::/7) are allowed.",
		);
	});
});
