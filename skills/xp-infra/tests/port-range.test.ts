import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

// Assert on the awk program shipped inside the probe, not a copy that drifts.
const source = readFileSync(join(import.meta.dir, "../scripts/xp-infra-port-remote"), "utf8");
const start = source.indexOf("listener_ports=\"$(printf");
const program = source.slice(
  source.indexOf("| awk '", start) + "| awk '".length,
  source.indexOf("')\" || exit 1", start),
);
async function occupied(rows: string[]) {
  const child = Bun.spawn(["awk", program], {
    stdin: new Blob([rows.join("\n") + "\n"]),
    stdout: "pipe",
    stderr: "pipe",
  });
  const [code, out] = await Promise.all([child.exited, new Response(child.stdout).text()]);
  return { code, out: out.trim() };
}
const row = (address: string) => `LISTEN 0 4096 ${address} 0.0.0.0:*`;
test("ordinary high ports are not rejected as out of range", async () => {
  // Regression: comparing the stripped port as a string made "9001" > "65535".
  for (const port of [8080, 9001, 58444, 65535]) {
    const result = await occupied([row(`0.0.0.0:${port}`)]);
    expect(result.code).toBe(0);
    expect(result.out).toBe(String(port));
  }
});
test("a port above 65535 still fails closed", async () => {
  expect((await occupied([row("0.0.0.0:65536")])).code).not.toBe(0);
});
test("listener and published port sources are merged and deduplicated", async () => {
  const result = await occupied([row("0.0.0.0:8080"), row("127.0.0.1:62416"), "8080", "443"]);
  expect(result.code).toBe(0);
  expect(result.out.split(",").sort()).toEqual(["443", "62416", "8080"]);
});
