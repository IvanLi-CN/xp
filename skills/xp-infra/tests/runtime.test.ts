import { expect, test } from "bun:test";
import { auditDecisions, normalizeBasic } from "../scripts/audit.ts";
import { preflight } from "../scripts/preflight.ts";
import { policy } from "../scripts/policy.ts";
import { basic, now, portRecords, validPolicy } from "./fixtures.ts";

const runtime = JSON.stringify({ runtime: "xp-proxy", state: "active", error: null });
test("xp-proxy audit uses selected service even with an inactive auxiliary Xray", () => {
  const observation = normalizeBasic(
    basic({
      runtime_json: runtime,
      health_json: JSON.stringify({
        status: "ok",
        xray: { status: "down" },
        cloudflared: { status: "up" },
      }),
    }),
    now,
  );
  expect(observation.ingress_kind).toBe("xp-proxy");
  expect(observation.ingress_runtime).toBe(true);
  observation.canary_enabled = observation.canary_certificate = true;
  observation.local_canary = observation.vless_ingress = true;
  observation.cloudflare_tunnel = observation.public_health = true;
  expect(auditDecisions(policy(validPolicy, ["fixture-node"]), observation).ok).toBe(true);
  for (const key of ["ingress_service", "ingress_public_listener", "vless_ingress"]) {
    expect(
      auditDecisions(policy(validPolicy, ["fixture-node"]), { ...observation, [key]: false }).ok,
    ).toBe(false);
  }
});
for (const patch of [
  { runtime: "unknown" },
  { state: "switching" },
  { state: "failed" },
  { error: "activation failed" },
]) {
  test(`reject invalid selected runtime ${JSON.stringify(patch)}`, () => {
    expect(() =>
      normalizeBasic(
        basic({
          runtime_json: JSON.stringify({
            runtime: "xp-proxy",
            state: "active",
            error: null,
            ...patch,
          }),
        }),
        now,
      ),
    ).toThrow();
  });
}
function nativePort(snapshot: unknown, overrides: Record<string, string> = {}) {
  return preflight(
    portRecords({
      runtime_json: runtime,
      state_b64: Buffer.from(
        JSON.stringify({ schema_version: 1, vless_endpoint_id: "managed-id" }),
      ).toString("base64"),
      ingress_ports: "4443",
      ingress_listener_pids: "123",
      ingress_runtime_pid: "123",
      ingress_service: "active",
      ingress_security: "1000 0000000000000000",
      ingress_api_b64: Buffer.from(JSON.stringify(snapshot)).toString("base64"),
      ...overrides,
    }),
    "fixture-node",
  );
}
const snapshot = { protocol_version: 2, generation: 3, listeners: [{ bind: "0.0.0.0:4443" }] };
test("native preflight preserves verified managed port", () => {
  expect(nativePort(snapshot)).toBe(4443);
});
test("native preflight rejects mismatched port, process, service and generation", () => {
  for (const patch of [
    { ingress_ports: "4444" },
    { ingress_listener_pids: "124" },
    { ingress_service: "inactive" },
  ])
    expect(() => nativePort(snapshot, patch)).toThrow();
  for (const patch of [
    { generation: 0 },
    { listeners: [] },
    { listeners: [...snapshot.listeners, ...snapshot.listeners] },
  ]) {
    expect(() => nativePort({ ...snapshot, ...patch })).toThrow();
  }
});
