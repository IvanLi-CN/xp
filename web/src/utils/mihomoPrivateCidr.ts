const PRIVATE_CIDR_ERROR =
	"Only RFC1918 IPv4 CIDRs or IPv6 ULA CIDRs (fc00::/7) are allowed.";

function parseIpv4Address(value: string): number[] | null {
	const parts = value.split(".");
	if (parts.length !== 4) return null;

	const octets = parts.map((part) => {
		if (!/^\d{1,3}$/.test(part)) return Number.NaN;
		return Number.parseInt(part, 10);
	});
	return octets.some((octet) => octet < 0 || octet > 255) ? null : octets;
}

function isPrivateIpv4(octets: number[], prefix: number): boolean {
	const [first, second] = octets;
	if (first === undefined || second === undefined) return false;
	return (
		(first === 10 && prefix >= 8) ||
		(first === 172 && second >= 16 && second <= 31 && prefix >= 12) ||
		(first === 192 && second === 168 && prefix >= 16)
	);
}

export function normalizeMihomoPrivateCidr(value: string): string {
	const trimmed = value.trim();
	if (!trimmed || trimmed.includes("/")) return trimmed;
	return `${trimmed}${trimmed.includes(":") ? "/128" : "/32"}`;
}

function firstIpv6Hextet(value: string): number | null {
	if (value.includes("%")) return null;
	try {
		const hostname = new URL(`http://[${value}]/`).hostname;
		if (!hostname.startsWith("[") || !hostname.endsWith("]")) return null;
		const first = hostname.slice(1, -1).split(":")[0];
		if (!first) return null;
		const hextet = Number.parseInt(first, 16);
		return Number.isInteger(hextet) ? hextet : null;
	} catch {
		return null;
	}
}

/**
 * Mirrors the accepted Mihomo policy CIDR boundary before a tag enters the draft.
 * The API remains authoritative and normalizes the network when it saves it.
 */
export function validateMihomoPrivateCidr(value: string): string | null {
	const [address, rawPrefix, ...rest] =
		normalizeMihomoPrivateCidr(value).split("/");
	if (!address || !rawPrefix || rest.length > 0) {
		return "CIDR must use address/prefix notation.";
	}
	if (!/^\d+$/.test(rawPrefix)) {
		return "CIDR prefix must be a whole number.";
	}

	const prefix = Number.parseInt(rawPrefix, 10);
	if (address.includes(":")) {
		const firstHextet = firstIpv6Hextet(address);
		if (firstHextet === null) {
			return "CIDR address must be a valid IPv4 or IPv6 literal.";
		}
		if (prefix > 128) return "IPv6 CIDR prefix must be between 0 and 128.";
		return firstHextet >= 0xfc00 && firstHextet <= 0xfdff && prefix >= 7
			? null
			: PRIVATE_CIDR_ERROR;
	}

	const octets = parseIpv4Address(address);
	if (!octets) return "CIDR address must be a valid IPv4 or IPv6 literal.";
	if (prefix > 32) return "IPv4 CIDR prefix must be between 0 and 32.";
	return isPrivateIpv4(octets, prefix) ? null : PRIVATE_CIDR_ERROR;
}
