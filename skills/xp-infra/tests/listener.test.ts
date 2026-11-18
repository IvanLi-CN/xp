import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const source = readFileSync(join(import.meta.dir, "../scripts/xp-infra-audit-remote"), "utf8");
const body = source.slice(
  source.indexOf("ingress_public_ports() {"),
  source.indexOf("read_runtime_status() {"),
);
async function ports(rows: string, runtime = "xp-proxy") {
  const child = Bun.spawn(["sh", "-s"], {
    env: { ...process.env, TEST_ROWS: rows, TEST_RUNTIME: runtime },
    stdin: new Blob([
      `set -eu
mode=host
ingress_runtime="$TEST_RUNTIME"
host_ingress_pid=123
container_ingress_token=
bounded_command() { printf '%s\\n' "$TEST_ROWS"; }
${body}
result="$(ingress_public_ports)" || exit 1
printf '%s' "$result"
`,
    ]),
    stdout: "pipe",
    stderr: "pipe",
  });
  const [code, out] = await Promise.all([child.exited, new Response(child.stdout).text()]);
  return { code, out };
}
const row = (owner: string, pid: number, address = "0.0.0.0:443") =>
  `LISTEN 0 128 ${address} 0.0.0.0:* users:(("${owner}",pid=${pid},fd=3))`;
test("audit selects only public listeners of the selected runtime and PID", async () => {
  for (const runtime of ["xp-proxy", "xray"]) {
    expect(
      await ports(
        [row(runtime, 123), row(runtime, 123, "127.0.0.1:10085"), row("other", 999)].join("\n"),
        runtime,
      ),
    ).toEqual({ code: 0, out: "443" });
  }
});
test("a later conflicting PID must not preserve an earlier accepted port", async () => {
  expect((await ports([row("xp-proxy", 123), row("xp-proxy", 124)].join("\n"))).code).not.toBe(0);
});
test("missing PID fails closed and unrelated owners yield no ingress evidence", async () => {
  expect(
    (await ports('LISTEN 0 128 0.0.0.0:443 0.0.0.0:* users:(("xp-proxy",fd=3))')).code,
  ).not.toBe(0);
  expect(await ports(row("xray", 123))).toEqual({ code: 0, out: "" });
});
