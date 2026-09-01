import { describe, expect, it } from "vitest";

import { fixtureCatalog } from "../fixture-policy/catalog";
import { validateMihomoPrivateCidr } from "./mihomoPrivateCidr";

describe("validateMihomoPrivateCidr", () => {
	it("accepts RFC1918 IPv4 and IPv6 ULA CIDRs", () => {
		expect(
			validateMihomoPrivateCidr(fixtureCatalog.address.privateCidr()),
		).toBeNull();
		expect(
			validateMihomoPrivateCidr(fixtureCatalog.address.privateIpv6Cidr()),
		).toBeNull();
	});

	it("rejects malformed addresses and invalid prefix lengths", () => {
		expect(
			validateMihomoPrivateCidr(
				fixtureCatalog.address.privateCidr().replace("/", ""),
			),
		).toBe("CIDR must use address/prefix notation.");
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
