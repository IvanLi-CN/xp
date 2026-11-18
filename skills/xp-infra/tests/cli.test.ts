import { afterEach, expect, test } from "bun:test";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { basic, privatePolicy, validPolicy } from "./fixtures.ts";

const dirs: string[] = [];
afterEach(() => {
  for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});
function fixture() {
  const dir = mkdtempSync(join(tmpdir(), "xp-cli-"));
  dirs.push(dir);
  mkdirSync(join(dir, "bin"));
  writeFileSync(join(dir, "hosts.txt"), "fixture-node\n");
  writeFileSync(join(dir, "deployment-policy.yaml"), privatePolicy);
  writeFileSync(
    join(dir, "response"),
    basic({
      runtime_mode: "container",
      host_ingress_pid: "",
      host_ingress_starttime: "",
      container_identity: "a".repeat(64),
      container_started_at: "2026-01-01T00:00:00Z",
      container_ingress_token: "99:12345",
      ddns_state_json: JSON.stringify({
        snapshot: { last_sync_at: new Date().toISOString(), status: "up", last_error: "" },
      }),
      health_json: '{"status":"ok","xray":{"status":"up"},"cloudflared":{"status":"down"}}',
      cloudflared_service: "false",
      cloudflare_enabled: "false",
      cloudflare_hostname: "",
      managed_default_state_json:
        '{"schema_version":1,"vless_endpoint_id":null,"ss_endpoint_id":null}',
      ingress_public_ports: "",
    }),
  );
  const ssh = `#!${process.execPath}
import { appendFileSync, readFileSync } from 'node:fs';
const dir = process.env.XP_FIXTURE;
appendFileSync(dir + '/calls', JSON.stringify(process.argv.slice(2)) + '\\n');
if (process.argv[2] === '-G') {
  const user = process.env.XP_FIXTURE_USER || 'operator';
  console.log('user ' + user + '\\nhostname fixture.invalid\\nport 22');
} else {
  const input = await Bun.stdin.text();
  appendFileSync(dir + '/scripts', input + '\\n');
  if (process.env.XP_FIXTURE_FAIL) {
    console.error('SECRET_CANARY_REMOTE_DIAGNOSTIC'); process.exit(1);
  }
  process.stdout.write(readFileSync(dir + '/response'));
}
`;
  writeFileSync(join(dir, "bin/ssh"), ssh);
  chmodSync(join(dir, "bin/ssh"), 0o755);
  return dir;
}
async function run(dir: string, args: string[], extra: NodeJS.ProcessEnv = {}) {
  const child = Bun.spawn(
    [process.execPath, join(import.meta.dir, "../scripts/xp-infra"), ...args],
    {
      env: { PATH: join(dir, "bin"), XP_INFRA_CONFIG_DIR: dir, XP_FIXTURE: dir, ...extra },
      stdout: "pipe",
      stderr: "pipe",
    },
  );
  const [out, err, code] = await Promise.all([
    new Response(child.stdout).text(),
    new Response(child.stderr).text(),
    child.exited,
  ]);
  return { out, err, code };
}
test("XP-01 invalid/missing policy stops before fake SSH records any call", async () => {
  for (const policy of [
    validPolicy.replace("owner_confirmed: true", "owner_confirmed: false"),
    "",
    validPolicy + "unsupported: true\n",
  ]) {
    const dir = fixture();
    writeFileSync(join(dir, "deployment-policy.yaml"), policy);
    for (const args of [
      ["audit", "fixture-node"],
      ["port", "preflight", "fixture-node"],
    ]) {
      const result = await run(dir, args);
      expect(result.code).not.toBe(0);
      expect(await Bun.file(join(dir, "calls")).exists()).toBe(false);
    }
  }
  const dir = fixture();
  rmSync(join(dir, "deployment-policy.yaml"));
  expect((await run(dir, ["audit", "fixture-node"])).code).not.toBe(0);
  expect(await Bun.file(join(dir, "calls")).exists()).toBe(false);
});
test("XP-03 operator denylist and missing user stop before remote probe", async () => {
  for (const user of ["root", "xp", " "]) {
    const dir = fixture();
    const result = await run(dir, ["audit", "fixture-node"], { XP_FIXTURE_USER: user });
    expect(result.code).not.toBe(0);
    expect(await Bun.file(join(dir, "scripts")).exists()).toBe(false);
  }
});
test("XP-05 fake SSH audit rejects failed or duplicate evidence", async () => {
  const dir = fixture();
  const passed = await run(dir, ["audit", "fixture-node"]);
  expect(passed.err).toBe("");
  expect(passed.code).toBe(0);
  expect(passed.out).toContain("audit: OK (fixture-node)");
  expect(passed.out).toContain("SKIP public_health");
  const calls = readFileSync(join(dir, "calls"), "utf8");
  expect(calls).toContain("fixture-node");
  expect(calls).toContain("'true' 'false'");
  writeFileSync(
    join(dir, "response"),
    readFileSync(join(dir, "response"), "utf8") + "xp_service=true\n",
  );
  const duplicate = await run(dir, ["audit", "fixture-node"]);
  expect(duplicate.code).not.toBe(0);
  expect(duplicate.out).not.toContain("audit: OK");
});
test("XP-05 partial DDNS family failure emits a sanitized diagnostic", async () => {
  const dir = fixture();
  writeFileSync(
    join(dir, "response"),
    readFileSync(join(dir, "response"), "utf8").replace(
      '"last_error":""',
      '"last_error":"ddns ipv6 probe failed"',
    ),
  );
  const result = await run(dir, ["audit", "fixture-node"]);
  expect(result.code).toBe(0);
  expect(result.out).toContain("PASS cloudflare_ddns");
  expect(result.out).toContain("INFO cloudflare_ddns_family_diagnostic=present");
  expect(result.out).toContain("audit: OK (fixture-node)");
});
test("XP-05 secret canaries in unrelated fields and diagnostics are never reported", async () => {
  const dir = fixture();
  writeFileSync(
    join(dir, "response"),
    readFileSync(join(dir, "response"), "utf8") + "administrator_token=<SECRET_CANARY_TOKEN>\n",
  );
  const result = await run(dir, ["audit", "fixture-node"]);
  expect(result.code).toBe(0);
  expect(result.out + result.err).not.toContain("SECRET_CANARY");
  const failed = await run(dir, ["audit", "fixture-node"], { XP_FIXTURE_FAIL: "1" });
  expect(failed.code).not.toBe(0);
  expect(failed.out + failed.err).not.toContain("SECRET_CANARY");
});
test("XP-06 standalone normalizer accepts bounded input with only Bun", async () => {
  const child = Bun.spawn(
    [process.execPath, join(import.meta.dir, "../scripts/xp-infra-audit-local"), "public-health"],
    {
      env: { PATH: "/nonexistent" },
      stdin: new Blob(['{"status":"ok","xray":{"status":"up"}}']),
      stdout: "pipe",
      stderr: "pipe",
    },
  );
  expect(await new Response(child.stdout).text()).toContain("public_health=true");
  expect(await child.exited).toBe(0);
});
