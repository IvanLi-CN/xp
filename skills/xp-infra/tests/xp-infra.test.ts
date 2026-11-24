import { afterEach, describe, expect, test } from "bun:test";
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  basicKeys,
  auditDecisions,
  normalizeBasic,
  normalizePublicHealth,
} from "../scripts/audit.ts";
import { bootSelection, parseArgs } from "../scripts/cli.ts";
import { policyYaml, records, strictJson, timestamp } from "../scripts/input.ts";
import { httpResult, inRange, probe, publicAddress, publicTarget } from "../scripts/network.ts";
import { inventory, policy } from "../scripts/policy.ts";
import { composeImageMatches, inboundPort, preflight, strictBase64 } from "../scripts/preflight.ts";
import { boundedInput, command } from "../scripts/process.ts";
import { basic, now, portRecords, privatePolicy, validPolicy } from "./fixtures.ts";

const dirs: string[] = [];
function temporary(): string {
  const dir = mkdtempSync(join(tmpdir(), "xp-tests-"));
  dirs.push(dir);
  return dir;
}
afterEach(() => {
  for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});
const hosts = ["fixture-node"];
describe("VER-FOUNDATION-XP-01 policy and inventory gates", () => {
  test("confirmed complete policy and approved override", () => {
    expect(policy(validPolicy, hosts, hosts[0]).managed_default_vless).toBe(true);
    expect(policy(privatePolicy, hosts, hosts[0]).direct_docker).toBe(true);
    expect(inventory("# neutral aliases\nfixture-node # operator alias\n")).toEqual(hosts);
  });
  test.each([
    validPolicy.replace("owner_confirmed: true", "owner_confirmed: false"),
    validPolicy.replace("schema_version: 1", "schema_version: 2"),
    validPolicy.replace("  direct_docker: false\n", ""),
    validPolicy.replace("  direct_docker: false", '  direct_docker: "false"'),
    validPolicy + "unknown: true\n",
    validPolicy.replace("  enabled: true", "  enabled: false"),
    privatePolicy.replace("owner_approved: true", "owner_approved: false"),
    privatePolicy.replace("reason: isolated private fixture", "reason: ''"),
    privatePolicy.replace("direct_docker: true", "direct_docker: false"),
    privatePolicy.replace("  fixture-node:", "  unregistered-node:"),
    validPolicy.replace("  zone: example.test", "  zone: EXAMPLE.test"),
  ])("invalid policy blocks: %#", (text) => {
    expect(() => policy(text, hosts, hosts[0])).toThrow();
  });
  test.each([
    "",
    "fixture-node\nfixture-node",
    "root@host",
    "fixture-node other",
    "--option",
    "node_name",
  ])("reject ambiguous inventory: %s", (text) => {
    expect(() => inventory(text)).toThrow();
  });
  test("unregistered host and unknown CLI layout fail before transport", () => {
    expect(() => policy(validPolicy, hosts, "other-node")).toThrow();
    for (const args of [
      ["status", "fixture-node", "extra"],
      ["port", "preflight", "fixture-node", "extra"],
      ["logs", "fixture-node", "--tail", "1;id"],
      ["tunnel", "fixture-node", "--local-port", "0"],
      ["tunnel", "fixture-node", "--metrics", "--ui"],
    ])
      expect(() => parseArgs(args)).toThrow();
  });
});

describe("VER-FOUNDATION-XP-02 strict inputs and normalization", () => {
  test.each([
    '{"x":1,"x":2}',
    '{"x":{"name":1,"na\\u006de":2}}',
    '{"x":[{"a":1,"a":2}]}',
    '{"n":9007199254740993}',
    '{"a":1,}',
    "[1,]",
    "null trailing",
  ])("reject invalid JSON: %s", (text) => {
    expect(() => strictJson(text)).toThrow();
  });
  test("valid JSON retains arrays, null and escaped keys", () => {
    expect(strictJson('{"a":[null,true,"a\\"b",1.5]}')).toEqual({ a: [null, true, 'a"b', 1.5] });
  });
  test.each([
    "a: 1\na: 2",
    "a: &anchor x\nb: *anchor",
    "a: !!str true",
    "a: {x: 1}",
    "a: |\n  body",
    "---\na: 1",
    "a:\n  <<: *x",
    "a:\n   b: true",
  ])("reject unsupported YAML: %s", (text) => {
    expect(() => policyYaml(text)).toThrow();
  });
  test("duplicate records and missing evidence fail", () => {
    expect(() => records("a=1\na=2\n")).toThrow();
    expect(() => records("a=1\nmalformed")).toThrow();
    for (const key of basicKeys)
      expect(() =>
        normalizeBasic(
          basic()
            .split("\n")
            .filter((line) => !line.startsWith(`${key}=`))
            .join("\n"),
          now,
        ),
      ).toThrow();
  });
  test.each([
    { ddns_enabled: "maybe" },
    { xp_service: "unknown" },
    { sync_node_meta: "" },
    { ddns_state_json: '{"snapshot":{"last_sync_at":"2026-02-30T00:00:00Z"}}' },
    { ingress_public_ports: "443,443" },
    { ingress_public_ports: "65536" },
    { ingress_public_ports: "443," },
    { managed_default_state_json: '{"schema_version":2}' },
    { managed_default_state_json: '{"schema_version":1,"vless_endpoint_id":7}' },
    { health_json: '{"status":"ok","xray":[]}' },
    { api_base: "https://user@fixture-xp.example.test" },
    { access_host: "Fixture-ep.example.test" },
    { geo_enabled: "true", geo_response_json: "{}" },
    { ddns_unmonitored_interval: "NaN" },
  ])("reject malformed observation: %#", (patch) => {
    expect(() => normalizeBasic(basic(patch), now)).toThrow();
  });
  test("freshness, certificate and future DDNS cannot become healthy", () => {
    expect(normalizeBasic(basic(), now).cloudflare_ddns).toBe(true);
    expect(normalizeBasic(basic(), now + 1000000).cloudflare_ddns).toBe(false);
    expect(normalizeBasic(basic(), now - 1000000).cloudflare_ddns).toBe(false);
    expect(normalizeBasic(basic(), Date.parse("2031-01-01T00:00:00Z")).canary_certificate).toBe(
      false,
    );
    expect(() => timestamp("2026-01-01", "time")).toThrow();
    expect(() => normalizePublicHealth('{"status":"ok","status":"bad"}')).toThrow();
  });
  test("DDNS up status remains healthy with a nonfatal family diagnostic", () => {
    const ddnsState = JSON.stringify({
      snapshot: {
        last_sync_at: "2025-12-31T23:59:30Z",
        status: "up",
        last_error: "ddns ipv6 probe failed",
        fast_mode_until: null,
      },
    });
    const observed = normalizeBasic(basic({ ddns_state_json: ddnsState }), now);
    expect(observed.cloudflare_ddns).toBe(true);
    expect(observed.cloudflare_ddns_diagnostic).toBe(true);
  });
  test("DDNS rejects malformed diagnostic types", () => {
    for (const last_error of [false, 123, {}, []]) {
      const ddnsState = JSON.stringify({
        snapshot: {
          last_sync_at: "2025-12-31T23:59:30Z",
          status: "up",
          last_error,
          fast_mode_until: null,
        },
      });
      expect(() => normalizeBasic(basic({ ddns_state_json: ddnsState }), now)).toThrow(
        "invalid DDNS last_error",
      );
    }
  });
});

describe("VER-FOUNDATION-XP-03 runtime identities", () => {
  test.each([
    { runtime_mode: "unknown" },
    { container_identity: "a".repeat(64) },
    { host_ingress_starttime: "" },
    { host_ingress_pid: "0" },
    {
      runtime_mode: "container",
      host_ingress_pid: "",
      host_ingress_starttime: "",
      container_identity: "abc",
    },
  ])("reject identity: %#", (patch) => {
    expect(() => normalizeBasic(basic(patch), now)).toThrow();
  });
  test("complete container identity is accepted", () => {
    expect(
      normalizeBasic(
        basic({
          runtime_mode: "container",
          host_ingress_pid: "",
          host_ingress_starttime: "",
          container_identity: "a".repeat(64),
          container_started_at: "2026-01-01T00:00:00.123456789Z",
          container_ingress_token: "99:12345",
        }),
        now,
      ).runtime_mode,
    ).toBe("container");
  });
  test("managed port requires matching process, service, inbound and port", () => {
    const api = {
      inbounds: [
        {
          tag: "vless-vision-fixture-id",
          receiverSettings: { type: "xray.app.proxyman.ReceiverConfig", portList: 4443 },
        },
      ],
    };
    const patch = {
      state_b64: Buffer.from('{"schema_version":1,"vless_endpoint_id":"fixture-id"}').toString(
        "base64",
      ),
      ingress_ports: "4443",
      ingress_listener_pids: "123",
      ingress_runtime_pid: "123",
      ingress_service: "active",
      ingress_api_b64: Buffer.from(JSON.stringify(api)).toString("base64"),
    };
    expect(preflight(portRecords(patch), "fixture-node")).toBe(4443);
    for (const mismatch of [
      { ingress_runtime_pid: "124" },
      { ingress_service: "inactive" },
      { ingress_ports: "4444" },
      { ingress_listener_pids: "123,124" },
      { ingress_api_b64: Buffer.from('{"inbounds":[]}').toString("base64") },
    ])
      expect(() => preflight(portRecords({ ...patch, ...mismatch }), "fixture-node")).toThrow();
  });
  test("protobuf receiver uses bounded exact-single port fields", () => {
    const receiver = (value: string) => ({
      receiverSettings: { type: "xray.app.proxyman.ReceiverConfig", value },
    });
    expect(
      inboundPort(
        receiver(Buffer.from([10, 8, 10, 6, 8, 0xdb, 0x22, 16, 0xdb, 0x22]).toString("base64")),
      ),
    ).toBe(4443);
    for (const bytes of [[10, 99], [0], [10, 2, 10, 0], [0xff, 0xff, 0xff]])
      expect(() => inboundPort(receiver(Buffer.from(bytes).toString("base64")))).toThrow();
  });
});

describe("VER-FOUNDATION-XP-04 public network and output boundaries", () => {
  test.each([
    "127.0.0.1",
    "10.0.0.1",
    "169.254.169.254",
    "100.64.0.1",
    "192.0.2.1",
    "198.51.100.1",
    "203.0.113.1",
    "224.0.0.1",
    "::1",
    "::ffff:127.0.0.1",
    "fc00::1",
    "2001:db8::1",
    "2002::1",
    "3fff::1",
  ])("deny nonpublic address: %s", (address) => {
    expect(publicAddress(address)).toBe(false);
  });
  test.each([
    "1.1.1.1",
    "8.8.8.8",
    "192.0.0.9",
    "2606:4700:4700::1111",
    "64:ff9b::808:808",
    "2001:1::1",
  ])("allow public address: %s", (address) => {
    expect(publicAddress(address)).toBe(true);
  });
  test("strict target, all DNS answers, and pinned address", async () => {
    const resolver = async () => ["1.1.1.1"];
    expect(
      (
        await publicTarget(
          "https://fixture.example.test:4443/generate_204",
          "/generate_204",
          resolver,
        )
      ).resolve,
    ).toBe("fixture.example.test:4443:1.1.1.1");
    for (const url of [
      "http://fixture.example.test/api/health",
      "https://127.0.0.1/api/health",
      "https://fixture.example.test/api/health?x=1",
      "https://u@fixture.example.test/api/health",
      "https://fixture.example.test/x/../api/health",
      "https://fixture.example.test:99999/api/health",
      "https://Fixture.example.test/api/health",
    ])
      await expect(publicTarget(url, "/api/health", resolver)).rejects.toThrow();
    for (const answers of [[], ["1.1.1.1", "127.0.0.1"], ["::ffff:10.0.0.1"]])
      await expect(
        publicTarget("https://fixture.example.test/api/health", "/api/health", async () => answers),
      ).rejects.toThrow();
    expect(inRange("2606:4700:4700::1111", "2606:4700::/32")).toBe(true);
  });
  test("redirects and missing headers fail", () => {
    expect(httpResult("HTTP/2 204 \r\nserver: fixture\r\n\r\n").status).toBe(204);
    for (const raw of [
      "HTTP/1.1 302 Found\r\nlocation: http://127.0.0.1\r\n\r\n",
      "{}",
      "HTTP/2 500 \r\n\r\n{}",
    ])
      expect(() => httpResult(raw)).toThrow();
  });
  test("controlled HTTPS endpoint preserves TLS, redirects and output bounds", async () => {
    const dir = temporary(),
      key = join(dir, "key.pem"),
      certificate = join(dir, "certificate.pem");
    await command(
      [
        "openssl",
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        key,
        "-out",
        certificate,
        "-days",
        "1",
        "-subj",
        "/CN=fixture.example.test",
        "-addext",
        "subjectAltName=DNS:fixture.example.test",
      ],
      { timeoutMs: 15000 },
    );
    let mode: "ok" | "redirect" | "oversize" = "ok";
    const server = Bun.serve({
      hostname: "127.0.0.1",
      port: 0,
      tls: { key: readFileSync(key), cert: readFileSync(certificate) },
      fetch() {
        if (mode === "redirect")
          return new Response(null, {
            status: 302,
            headers: { location: "https://127.0.0.1/private" },
          });
        return new Response(
          mode === "oversize" ? "x".repeat(65537) : '{"status":"ok","xray":{"status":"up"}}',
        );
      },
    });
    const prior = process.env.CURL_CA_BUNDLE;
    process.env.CURL_CA_BUNDLE = certificate;
    const target = {
      url: `https://fixture.example.test:${server.port}/api/health`,
      host: "fixture.example.test",
      port: server.port!,
      address: "127.0.0.1",
      resolve: `fixture.example.test:${server.port}:127.0.0.1`,
    };
    try {
      // The synthetic target is injected only here; publicTarget rejects loopback.
      expect(normalizePublicHealth((await probe(target)).body).public_health).toBe(true);
      mode = "redirect";
      await expect(probe(target)).rejects.toThrow();
      mode = "oversize";
      await expect(probe(target)).rejects.toThrow();
    } finally {
      server.stop(true);
      if (prior === undefined) delete process.env.CURL_CA_BUNDLE;
      else process.env.CURL_CA_BUNDLE = prior;
    }
  }, 20000);
  test("process limits stdout, diagnostics, exit status and wall time", async () => {
    await expect(
      command([process.execPath, "-e", 'process.stdout.write("x".repeat(65537))']),
    ).rejects.toThrow("exceeds limit");
    await expect(
      command([process.execPath, "-e", 'process.stderr.write("s".repeat(65537))']),
    ).rejects.toThrow("exceed limit");
    await expect(command([process.execPath, "-e", "process.exit(3)"])).rejects.toThrow();
    const start = Date.now();
    await expect(command(["sh", "-c", "sleep 20 & wait"], { timeoutMs: 100 })).rejects.toThrow(
      "timed out",
    );
    expect(Date.now() - start).toBeLessThan(3000);
  });
  test("stdin parser stops on overflow, invalid UTF-8 and stalls", async () => {
    await expect(boundedInput(new Blob(["x".repeat(65537)]).stream())).rejects.toThrow();
    await expect(boundedInput(new Blob([new Uint8Array([255])]).stream())).rejects.toThrow();
    await expect(boundedInput(new ReadableStream({ start() {} }), 20)).rejects.toThrow("timed out");
  });
});

describe("VER-FOUNDATION-XP-05 audit decisions and bootstrap selection", () => {
  test("complete audit success and each failed required fact", () => {
    const p = policy(validPolicy, hosts, hosts[0]);
    const observed = {
      ...normalizeBasic(basic(), now),
      public_health: true,
      local_canary: true,
      vless_ingress: true,
    };
    expect(auditDecisions(p, observed).ok).toBe(true);
    for (const key of [
      "xp_service",
      "ingress_service",
      "cloudflared_service",
      "cloudflare_tunnel",
      "cloudflare_ddns",
      "ip_geo_enrichment",
      "cluster_sync",
      "local_health",
      "ingress_runtime",
      "public_health",
      "local_canary",
      "vless_ingress",
    ])
      expect(auditDecisions(p, { ...observed, [key]: false }).ok).toBe(false);
    expect(auditDecisions(p, { ...observed, cluster_role: "unknown" }).ok).toBe(false);
  });
  test("private exception requires container and explicit feature disable", () => {
    const p = policy(privatePolicy, hosts, hosts[0]);
    const o = {
      ...normalizeBasic(basic(), now),
      runtime_mode: "container",
      cloudflare_tunnel: false,
      managed_default_vless: false,
    };
    expect(auditDecisions(p, o).ok).toBe(true);
    expect(auditDecisions(p, { ...o, runtime_mode: "host" }).ok).toBe(false);
  });
  test("preflight preserves managed ports and selects stable available candidates", () => {
    const port = preflight(portRecords(), hosts[0]);
    expect(port).toBe(preflight(portRecords(), hosts[0]));
    expect(port).toBeGreaterThanOrEqual(1024);
    expect(port >= 32768 && port <= 60999).toBe(false);
    expect([8080, 443, 22].includes(port)).toBe(false);
    expect(() => preflight(portRecords({ reserved_ports: "1-65535" }), hosts[0])).toThrow(
      "no bindable",
    );
    expect(() => preflight(portRecords({ ingress_ports: "4443" }), hosts[0])).toThrow();
    for (const patch of [
      { occupied_ports: "22,99999" },
      { ephemeral_range: "60000 30000" },
      { ingress_security: "root ALL" },
      { state_b64: "!invalid" },
    ])
      expect(() => preflight(portRecords(patch), hosts[0])).toThrow();
  });
  test("canonical base64 and exact one compose image", () => {
    expect(strictBase64("eA==").toString()).toBe("x");
    expect(() => strictBase64("eB==")).toThrow();
    expect(composeImageMatches("\nfixture/image:tag\n", "fixture/image:tag")).toBe(true);
    for (const input of [
      "",
      "fixture/image:other",
      "fixture/image:tag\nfixture/image:tag",
      " fixture/image:tag",
    ])
      expect(composeImageMatches(input, "fixture/image:tag")).toBe(false);
  });
});

describe("VER-FOUNDATION-XP-06 fresh local entrypoints", () => {
  test("direct Bun shebang and retained POSIX probes", async () => {
    const scripts = join(import.meta.dir, "../scripts");
    expect(readFileSync(join(scripts, "xp-infra"), "utf8").startsWith("#!/usr/bin/env bun\n")).toBe(
      true,
    );
    for (const file of [
      "xp-infra-audit-remote",
      "xp-infra-audit-vless-remote",
      "xp-infra-port-remote",
      "xp-infra-inventory-remote",
    ]) {
      const source = readFileSync(join(scripts, file), "utf8");
      expect(source.startsWith("#!/bin/sh\n")).toBe(true);
      expect(source).not.toMatch(/\b(?:bun|ruby|python[23]?)\b/);
      await command(["sh", "-n", join(scripts, file)]);
    }
    expect(
      await command([process.execPath, join(scripts, "xp-infra"), "--help"], {
        env: { PATH: "/usr/bin:/bin" },
      }),
    ).toContain("read-only");
  });
  test("policy command runs with no Ruby/Python or transport", async () => {
    const dir = temporary();
    writeFileSync(join(dir, "hosts.txt"), "fixture-node\n");
    writeFileSync(join(dir, "deployment-policy.yaml"), validPolicy);
    expect(
      await command(
        [process.execPath, join(import.meta.dir, "../scripts/xp-infra"), "policy", "check"],
        { env: { XP_INFRA_CONFIG_DIR: dir, PATH: dir } },
      ),
    ).toBe("policy: OK\n");
  });
});
describe("BOOT-RUNTIME-XP-01 runtime boot selection evidence", () => {
  const evidence = (patch: Record<string, string> = {}) =>
    Object.entries({
      runtime: "xp-proxy",
      boot_manager: "openrc",
      boot_xray: "true",
      boot_xp_proxy: "false",
      boot_selected: "false",
      boot_other: "true",
      boot_consistent: "false",
      ...patch,
    })
      .map(([key, value]) => `${key}=${value}`)
      .join("\n");
  test("consistent selection passes for both runtimes and managers", () => {
    for (const [runtime, manager] of [
      ["xp-proxy", "openrc"],
      ["xray", "systemd"],
    ])
      expect(
        bootSelection(
          evidence({
            runtime,
            boot_manager: manager,
            boot_xray: runtime === "xray" ? "true" : "false",
            boot_xp_proxy: runtime === "xp-proxy" ? "true" : "false",
            boot_selected: "true",
            boot_other: "false",
            boot_consistent: "true",
          }),
        ),
      ).toEqual({
        kind: "host",
        runtime,
        manager,
        selectedBoot: true,
        otherBoot: false,
        consistent: true,
      });
  });
  test("a selected runtime missing at boot is reported inconsistent", () => {
    expect(bootSelection(evidence())).toEqual({
      kind: "host",
      runtime: "xp-proxy",
      manager: "openrc",
      selectedBoot: false,
      otherBoot: true,
      consistent: false,
    });
  });
  test("container-managed nodes report no host boot selection", () => {
    expect(
      bootSelection(
        ["runtime=xray", "boot_manager=container", "boot_consistent=not_applicable"].join("\n"),
      ),
    ).toEqual({ kind: "container", runtime: "xray" });
    expect(() =>
      bootSelection(
        [
          "runtime=xray",
          "boot_manager=container",
          "boot_consistent=not_applicable",
          "boot_xray=true",
        ].join("\n"),
      ),
    ).toThrow();
  });
  test.each([
    evidence({ boot_consistent: "true" }),
    evidence({ boot_other: "false" }),
    evidence({ runtime: "xray" }),
    evidence({ boot_xray: "false", boot_xp_proxy: "true" }),
    evidence({ boot_manager: "runit" }),
    evidence({ boot_selected: "yes" }),
    evidence({ boot_consistent: "true" }).replace("boot_other=true\n", ""),
  ])("rejecting contradictory or incomplete evidence: %#", (text) => {
    expect(() => bootSelection(text)).toThrow();
  });
  test("runtime command requires only an alias", () => {
    expect(parseArgs(["runtime", "fixture-node"])).toEqual({
      command: "runtime",
      host: "fixture-node",
    });
    expect(parseArgs(["runtime", "--all"])).toEqual({ command: "runtime-all" });
    for (const args of [
      ["runtime", "fixture-node", "extra"],
      ["runtime", "--all", "fixture-node"],
      ["runtime", "fixture-node", "--json"],
    ])
      expect(() => parseArgs(args)).toThrow();
  });
});
