import { isIP } from "node:net";
import { dnsName, ensure, integer, strictJson } from "./input.ts";
import { command } from "./process.ts";

function ip(value: string): { bits: number; value: bigint } {
  const family = isIP(value);
  ensure(family, "invalid IP address");
  if (family === 4)
    return { bits: 32, value: value.split(".").reduce((n, part) => (n << 8n) | BigInt(part), 0n) };
  ensure(!value.includes("%"), "scoped IP address is prohibited");
  let normalized = value;
  if (value.includes(".")) {
    const last = value.lastIndexOf(":"),
      v4 = ip(value.slice(last + 1)).value;
    normalized =
      `${value.slice(0, last)}:${(v4 >> 16n).toString(16)}` + `:${(v4 & 65535n).toString(16)}`;
  }
  const parts = normalized.split("::"),
    left = parts[0] ? parts[0].split(":") : [],
    right = parts[1] ? parts[1].split(":") : [];
  const words =
    parts.length === 1
      ? left
      : [...left, ...Array(8 - left.length - right.length).fill("0"), ...right];
  ensure(words.length === 8, "invalid IPv6 address");
  return { bits: 128, value: words.reduce((n, part) => (n << 16n) | BigInt(`0x${part}`), 0n) };
}
export function inRange(address: string, cidr: string): boolean {
  const [base, mask] = cidr.split("/");
  const network = ip(base),
    candidate = ip(address);
  const prefix = mask === undefined ? network.bits : integer(mask, 0, network.bits, "network mask");
  const shift = BigInt(network.bits - prefix);
  return candidate.bits === network.bits && candidate.value >> shift === network.value >> shift;
}
const denied = [
  "0.0.0.0/8",
  "10.0.0.0/8",
  "100.64.0.0/10",
  "127.0.0.0/8",
  "169.254.0.0/16",
  "172.16.0.0/12",
  "192.0.0.0/24",
  "192.0.2.0/24",
  "192.168.0.0/16",
  "192.88.99.0/24",
  "198.18.0.0/15",
  "198.51.100.0/24",
  "203.0.113.0/24",
  "224.0.0.0/4",
  "240.0.0.0/4",
  "2001::/23",
  "2001:db8::/32",
  "2002::/16",
  "3fff::/20",
];
const exceptions = [
  "192.0.0.9",
  "192.0.0.10",
  "64:ff9b::/96",
  "2001:1::1/128",
  "2001:1::2/128",
  "2001:1::3/128",
  "2001:3::/32",
  "2001:4:112::/48",
  "2001:20::/28",
  "2001:30::/28",
];
export function publicAddress(address: string): boolean {
  const parsed = ip(address);
  return (
    exceptions.some((range) => inRange(address, range)) ||
    (!denied.some((range) => inRange(address, range)) &&
      (parsed.bits === 32 || inRange(address, "2000::/3")))
  );
}
export type Resolver = (host: string) => Promise<string[]>;
export interface PublicTarget {
  url: string;
  host: string;
  port: number;
  address: string;
  resolve: string;
}
async function resolveAddresses(host: string): Promise<string[]> {
  // getaddrinfo itself is not cancellable. A short-lived Bun child gives DNS
  // the same enforceable process-group deadline as the system probes.
  const raw = await command(
    [
      process.execPath,
      "-e",
      'import { lookup } from "node:dns/promises"; ' +
        "try { const addresses = await lookup(process.argv[1], { all: true }); " +
        "console.log(JSON.stringify(addresses.map(item => item.address))); } " +
        "catch { process.exit(1); }",
      host,
    ],
    { timeoutMs: 10000 },
  );
  const addresses = strictJson(raw);
  ensure(
    Array.isArray(addresses) &&
      addresses.every((value): value is string => typeof value === "string"),
    "invalid DNS response",
  );
  return addresses;
}
export async function publicTarget(
  target: string,
  path: "/api/health" | "/generate_204",
  resolver: Resolver = resolveAddresses,
): Promise<PublicTarget> {
  // Validate the original representation before URL can normalize paths or hosts.
  const match = /^https:\/\/([a-z0-9.-]+)(?::([1-9][0-9]*))?(\/[^?#\s]*)$/.exec(target);
  ensure(
    match && dnsName(match[1]) && !isIP(match[1]) && match[3] === path,
    "unsafe public target",
  );
  const port = match[2] ? integer(match[2], 1, 65535, "target port") : 443;
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const addresses = await Promise.race([
      resolver(match[1]),
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new Error("DNS resolution timed out")), 10000);
      }),
    ]);
    ensure(
      addresses.length > 0 && addresses.length <= 64 && addresses.every(publicAddress),
      "unsafe or missing public DNS addresses",
    );
    const address = addresses[0];
    return {
      url: target,
      host: match[1],
      port,
      address,
      resolve: `${match[1]}:${port}:${isIP(address) === 6 ? `[${address}]` : address}`,
    };
  } finally {
    clearTimeout(timer);
  }
}
export interface HttpResult {
  status: number;
  headers: string;
  body: string;
}
export function httpResult(text: string): HttpResult {
  const separator = text.indexOf("\r\n\r\n");
  ensure(separator >= 0, "missing HTTP headers");
  const headers = text.slice(0, separator),
    status = /^HTTP\/(?:1\.[01]|2|3) ([0-9]{3})(?: |\r?$)/m.exec(headers);
  ensure(
    status && Number(status[1]) >= 200 && Number(status[1]) < 300,
    "HTTP probe failed or redirected",
  );
  ensure(!/^[^:\r\n]+:[^\r\n]*[\x00-\x08\x0b\x0c\x0e-\x1f]/m.test(headers), "invalid HTTP headers");
  return { status: Number(status[1]), headers, body: text.slice(separator + 4) };
}
export async function probe(target: PublicTarget): Promise<HttpResult> {
  const raw = await command(
    [
      "curl",
      "-fsS",
      "--include",
      "--noproxy",
      "*",
      "--proto",
      "=https",
      "--max-redirs",
      "0",
      "--max-time",
      "15",
      "--max-filesize",
      "65536",
      "--resolve",
      target.resolve,
      "--",
      target.url,
    ],
    { timeoutMs: 20000 },
  );
  return httpResult(raw);
}
export async function cloudflareAddress(address: string): Promise<boolean> {
  const responses = await Promise.all(
    ["v4", "v6"].map(async (family) => {
      const raw = await command(
        [
          "curl",
          "-fsS",
          "--include",
          "--noproxy",
          "*",
          "--proto",
          "=https",
          "--max-redirs",
          "0",
          "--max-time",
          "15",
          "--max-filesize",
          "65536",
          "--",
          `https://www.cloudflare.com/ips-${family}`,
        ],
        { timeoutMs: 20000 },
      );
      return httpResult(raw).body.trim().split(/\r?\n/);
    }),
  );
  const ranges = responses.flat();
  ensure(
    ranges.length > 0 &&
      ranges.every((range) => range.includes("/") && inRange(range.split("/")[0], range)),
    "invalid Cloudflare ranges",
  );
  return ranges.some((range) => inRange(address, range));
}
