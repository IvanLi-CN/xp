import {
  dnsName,
  encodeRecords,
  ensure,
  explicitBoolean,
  integer,
  object,
  records,
  strictJson,
  timestamp,
  type ObjectValue,
} from "./input.ts";
import type { EffectivePolicy } from "./policy.ts";

export const basicKeys = [
  "runtime_json",
  "xp_service",
  "ingress_service",
  "cloudflared_service",
  "runtime_mode",
  "container_identity",
  "container_started_at",
  "container_ingress_token",
  "host_ingress_pid",
  "host_ingress_starttime",
  "ddns_enabled",
  "ddns_monitor_mode",
  "ddns_restart_mode",
  "ddns_fast_interval",
  "ddns_monitored_interval",
  "ddns_unmonitored_interval",
  "ddns_state_json",
  "geo_enabled",
  "geo_origin",
  "geo_response_json",
  "health_json",
  "cluster_json",
  "node_name",
  "api_base",
  "access_host",
  "cloudflare_enabled",
  "cloudflare_hostname",
  "cloudflare_settings_json",
  "managed_default_state_json",
  "sync_node_meta",
  "ingress_public_ports",
] as const;
export type RuntimeIdentity =
  | { mode: "host"; pid: string; starttime: string }
  | { mode: "container"; id: string; startedAt: string; ingressProcessIdentity: string };
export type Observation = Record<string, string | boolean>;
function jsonObject(text: string, name: string, required = false): ObjectValue {
  ensure(text !== "" || !required, `missing ${name}`);
  return text === "" ? {} : object(strictJson(text), name);
}
function nested(parent: ObjectValue, key: string): ObjectValue {
  return parent[key] == null ? {} : object(parent[key], key);
}
export function identity(r: Record<string, string>): RuntimeIdentity {
  if (r.runtime_mode === "host") {
    ensure(
      !r.container_identity && !r.container_started_at && !r.container_ingress_token,
      "contradictory host identity",
    );
    ensure(
      /^[1-9][0-9]*$/.test(r.host_ingress_pid) && /^[1-9][0-9]*$/.test(r.host_ingress_starttime),
      "invalid host identity",
    );
    return { mode: "host", pid: r.host_ingress_pid, starttime: r.host_ingress_starttime };
  }
  ensure(
    r.runtime_mode === "container" &&
      /^[0-9a-f]{64}$/.test(r.container_identity) &&
      /^[1-9][0-9]*:[1-9][0-9]*$/.test(r.container_ingress_token),
    "invalid container identity",
  );
  timestamp(r.container_started_at, "container start time");
  ensure(!r.host_ingress_pid && !r.host_ingress_starttime, "contradictory container identity");
  return {
    mode: "container",
    id: r.container_identity,
    startedAt: r.container_started_at,
    ingressProcessIdentity: r.container_ingress_token,
  };
}
export function normalizeBasic(text: string, now = Date.now()): Observation {
  const r = records(text, basicKeys);
  identity(r);
  const runtime = object(strictJson(r.runtime_json), "runtime state");
  ensure(
    ["xray", "xp-proxy"].includes(String(runtime.runtime)) &&
      runtime.state === "active" &&
      runtime.error == null,
    "ingress runtime not active",
  );
  const health = jsonObject(r.health_json, "health_json", true);
  const cluster = jsonObject(r.cluster_json, "cluster_json", true);
  const ddnsEnabled = explicitBoolean(r.ddns_enabled, "ddns_enabled");
  const ddns = jsonObject(r.ddns_state_json, "ddns_state_json", ddnsEnabled);
  const snapshot = ddnsEnabled ? object(ddns.snapshot, "ddns snapshot") : {};
  const fastUntil =
    snapshot.fast_mode_until == null || snapshot.fast_mode_until === ""
      ? undefined
      : timestamp(snapshot.fast_mode_until, "fast_mode_until");
  const monitor = r.ddns_monitor_mode || r.ddns_restart_mode;
  const intervalValue =
    fastUntil !== undefined && fastUntil > now
      ? r.ddns_fast_interval
      : monitor && monitor !== "none"
        ? r.ddns_monitored_interval
        : r.ddns_unmonitored_interval;
  const interval =
    intervalValue === ""
      ? fastUntil !== undefined && fastUntil > now
        ? 30
        : monitor && monitor !== "none"
          ? 300
          : 60
      : Math.min(integer(intervalValue, 0, Number.MAX_SAFE_INTEGER, "DDNS interval"), 3600);
  const lastSync = ddnsEnabled ? timestamp(snapshot.last_sync_at, "last_sync_at") : undefined;
  const lastError = snapshot.last_error;
  ensure(lastError == null || typeof lastError === "string", "invalid DDNS last_error");
  // One synced address family is healthy; retain the nonfatal family diagnostic.
  const ddnsOk =
    ddnsEnabled &&
    lastSync !== undefined &&
    lastSync <= now &&
    now - lastSync <= interval * 2000 + 60000 &&
    snapshot.status === "up";
  const geoEnabled = explicitBoolean(r.geo_enabled, "geo_enabled", false);
  const geo = jsonObject(r.geo_response_json, "geo_response_json", geoEnabled);
  ensure(
    !geoEnabled || (typeof geo.country === "string" && geo.country.length > 0),
    "invalid geo country",
  );
  const api = /^https:\/\/([a-z0-9.-]+)(?::([1-9][0-9]*))?\/?$/.exec(r.api_base);
  ensure(api && dnsName(api[1]), "api_base must be an HTTPS origin");
  if (api[2]) integer(api[2], 1, 65535, "api port");
  ensure(dnsName(r.access_host), "invalid access_host");
  const cloudflareEnabled = explicitBoolean(r.cloudflare_enabled, "cloudflare_enabled");
  const settings = jsonObject(r.cloudflare_settings_json, "cloudflare settings");
  if (r.cloudflare_settings_json && settings.enabled !== undefined)
    ensure(typeof settings.enabled === "boolean", "invalid tunnel enabled flag");
  const enabled = r.cloudflare_settings_json ? settings.enabled === true : cloudflareEnabled;
  const hostname = r.cloudflare_settings_json
    ? String(settings.hostname ?? "")
    : r.cloudflare_hostname;
  const declaration =
    enabled && dnsName(hostname) && hostname === api[1]
      ? "aligned"
      : !enabled && hostname === ""
        ? "missing"
        : "mismatch";
  const state = jsonObject(r.managed_default_state_json, "managed state");
  if (r.managed_default_state_json) {
    ensure(state.schema_version === 1, "unsupported managed state schema");
    for (const key of ["vless_endpoint_id", "ss_endpoint_id"])
      ensure(
        state[key] == null || (typeof state[key] === "string" && state[key] !== ""),
        `invalid ${key}`,
      );
  }
  const ports =
    r.ingress_public_ports === ""
      ? []
      : r.ingress_public_ports.split(",").map((p) => integer(p, 1, 65535, "public port"));
  ensure(new Set(ports).size === ports.length, "duplicate public port");
  const xray = nested(health, "xray"),
    cloudflared = nested(health, "cloudflared"),
    canary = nested(health, "vless_https_canary");
  if (canary.enabled !== undefined)
    ensure(typeof canary.enabled === "boolean", "invalid canary enabled flag");
  const certExpiry =
    canary.cert_not_after == null || canary.cert_not_after === ""
      ? undefined
      : timestamp(canary.cert_not_after, "certificate expiry");
  const canaryBind = /^(?:127\.0\.0\.1|\[::1\]):([1-9][0-9]*)$/.exec(String(canary.bind ?? ""));
  const canaryPort = canaryBind ? integer(canaryBind[1], 1, 65535, "canary port") : undefined;
  const out: Observation = {
    runtime_mode: r.runtime_mode,
    container_identity: r.container_identity,
    container_started_at: r.container_started_at,
    container_ingress_token: String(r.container_ingress_token),
    host_ingress_pid: r.host_ingress_pid,
    host_ingress_starttime: r.host_ingress_starttime,
    xp_service: explicitBoolean(r.xp_service, "xp_service"),
    ingress_service: explicitBoolean(r.ingress_service, "ingress_service"),
    cloudflared_service: explicitBoolean(r.cloudflared_service, "cloudflared_service"),
    cloudflare_ddns_enabled: ddnsEnabled,
    cloudflare_ddns: ddnsOk,
    cloudflare_ddns_diagnostic: typeof lastError === "string" && lastError !== "",
    ip_geo_enrichment:
      geoEnabled &&
      (r.geo_origin || "https://api.country.is") === "https://api.country.is" &&
      typeof geo.country === "string" &&
      geo.country !== "",
    local_health: health.status === "ok",
    ingress_runtime: runtime.runtime === "xp-proxy" ? health.status === "ok" : xray.status === "up",
    ingress_kind: String(runtime.runtime),
    cloudflare_tunnel: cloudflared.status === "up",
    canary_enabled: canary.enabled === true,
    canary_certificate:
      canary.enabled === true &&
      canary.last_error == null &&
      certExpiry !== undefined &&
      certExpiry > now,
    cluster_role:
      typeof cluster.role === "string" && ["leader", "follower"].includes(cluster.role)
        ? cluster.role
        : "missing",
    node_name: r.node_name,
    node_name_valid: /^[a-z0-9](?:[a-z0-9-]{0,58}[a-z0-9])?$/.test(r.node_name),
    api_host: api[1],
    access_host: r.access_host,
    cluster_sync: explicitBoolean(r.sync_node_meta, "sync_node_meta"),
    tunnel_declaration: declaration,
    managed_default_vless: state.vless_endpoint_id != null,
    managed_default_ss2022: state.ss_endpoint_id != null,
    ingress_public_listener: ports.length === 1,
    public_health_target: `${r.api_base.replace(/\/$/, "")}/api/health`,
  };
  if (canaryPort) out.canary_local_target = `https://${r.access_host}:${canaryPort}/generate_204`;
  if (state.vless_endpoint_id != null && ports.length === 1)
    out.vless_public_target = `https://${r.access_host}:${ports[0]}/generate_204`;
  return out;
}
export function normalizePublicHealth(text: string, runtime = "xray"): Observation {
  const health = object(strictJson(text), "public health");
  return {
    public_health:
      health.status === "ok" && (runtime === "xp-proxy" || nested(health, "xray").status === "up"),
    public_health_json_ok: health.status === "ok",
  };
}
export function privateNode(p: EffectivePolicy): boolean {
  return p.direct_docker && !p.cloudflare_tunnel && !p.managed_default_vless;
}
export function auditDecisions(
  p: EffectivePolicy,
  o: Observation,
): { ok: boolean; lines: string[] } {
  const lines: string[] = [];
  let ok = true;
  const check = (name: string, actual: unknown, expected: boolean = true) => {
    const passed = actual === expected;
    lines.push(`${passed ? "PASS" : "FAIL"} ${name}`);
    if (!passed) ok = false;
  };
  const skip = (name: string) => lines.push(`SKIP ${name} (approved exception)`);
  if (privateNode(p)) {
    check("private_docker_runtime_mode", o.runtime_mode === "container");
    skip("public_hostname_convention");
  } else
    check(
      "public_hostname_convention",
      o.node_name_valid === true &&
        o.api_host === `${o.node_name}-xp.${p.public_hostname_zone}` &&
        o.access_host === `${o.node_name}-ep.${p.public_hostname_zone}`,
    );
  for (const key of [
    "cloudflare_tunnel",
    "ip_geo_enrichment",
    "managed_default_vless",
    "managed_default_ss2022",
  ] as const)
    check(key, o[key], p[key]);
  check(
    "cloudflare_ddns",
    p.cloudflare_ddns
      ? o.cloudflare_ddns_enabled === true && o.cloudflare_ddns === true
      : o.cloudflare_ddns_enabled,
    p.cloudflare_ddns,
  );
  if (o.cloudflare_ddns_diagnostic === true)
    lines.push("INFO cloudflare_ddns_family_diagnostic=present");
  for (const key of [
    "xp_service",
    "ingress_service",
    "cluster_sync",
    "local_health",
    "ingress_runtime",
  ])
    check(key, o[key]);
  if (p.cloudflare_tunnel) {
    check("cloudflared_service", o.cloudflared_service);
    check("tunnel_declaration", o.tunnel_declaration === "aligned");
  } else skip("cloudflared_service");
  for (const key of [
    "ingress_public_listener",
    "canary_enabled",
    "canary_certificate",
    "local_canary",
    "vless_ingress",
  ]) {
    if (p.managed_default_vless) check(key, o[key]);
    else skip(key);
  }
  if (privateNode(p)) skip("public_health");
  else check("public_health", o.public_health);
  check("cluster_role", o.cluster_role === "leader" || o.cluster_role === "follower");
  return { ok, lines };
}
export { encodeRecords };
