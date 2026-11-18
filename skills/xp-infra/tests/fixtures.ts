import { encodeRecords } from "../scripts/input.ts";
export const now = Date.parse("2026-01-01T00:00:00Z");
export const validPolicy = `schema_version: 1
owner_confirmed: true
geo:
  enabled: true
  origin: https://api.country.is
  owner_authorized_third_party_lookup: true
public_hostname:
  zone: example.test
defaults:
  cloudflare_tunnel: true
  cloudflare_ddns: true
  ip_geo_enrichment: true
  managed_default_vless: true
  managed_default_ss2022: false
  direct_docker: false
  vless_port: automatic_preflight
node_overrides: {}
`;
export const privatePolicy = validPolicy.replace(
  "node_overrides: {}",
  `node_overrides:
  fixture-node:
    owner_approved: true
    reason: isolated private fixture
    defaults:
      cloudflare_tunnel: false
      managed_default_vless: false
      direct_docker: true`,
);
export function basic(overrides: Partial<Record<string, string>> = {}): string {
  return encodeRecords({
    runtime_json: '{"runtime":"xray","state":"active","error":null}',
    xp_service: "true",
    ingress_service: "true",
    cloudflared_service: "true",
    runtime_mode: "host",
    container_identity: "",
    container_started_at: "",
    container_ingress_token: "",
    host_ingress_pid: "123",
    host_ingress_starttime: "456",
    ddns_enabled: "true",
    ddns_monitor_mode: "none",
    ddns_restart_mode: "",
    ddns_fast_interval: "30",
    ddns_monitored_interval: "300",
    ddns_unmonitored_interval: "60",
    ddns_state_json: JSON.stringify({
      snapshot: {
        last_sync_at: "2025-12-31T23:59:30Z",
        status: "up",
        last_error: "",
        fast_mode_until: null,
      },
    }),
    geo_enabled: "true",
    geo_origin: "https://api.country.is",
    geo_response_json: '{"country":"ZZ"}',
    health_json: JSON.stringify({
      status: "ok",
      xray: { status: "up" },
      cloudflared: { status: "up" },
      vless_https_canary: {
        enabled: true,
        last_error: null,
        cert_not_after: "2030-01-01T00:00:00Z",
        bind: "127.0.0.1:5555",
      },
    }),
    cluster_json: '{"role":"leader"}',
    node_name: "fixture",
    api_base: "https://fixture-xp.example.test",
    access_host: "fixture-ep.example.test",
    cloudflare_enabled: "true",
    cloudflare_hostname: "fixture-xp.example.test",
    cloudflare_settings_json: "",
    managed_default_state_json:
      '{"schema_version":1,"vless_endpoint_id":"fixture-id","ss_endpoint_id":null}',
    sync_node_meta: "true",
    ingress_public_ports: "4443",
    ...Object.fromEntries(
      Object.entries(overrides).filter(
        (entry): entry is [string, string] => entry[1] !== undefined,
      ),
    ),
  });
}
export function portRecords(overrides: Partial<Record<string, string>> = {}): string {
  return encodeRecords({
    runtime_json: '{"runtime":"xray","state":"active","error":null}',
    state_b64: Buffer.from("{}").toString("base64"),
    occupied_ports: "22,443",
    ingress_ports: "",
    ingress_listener_pids: "",
    ephemeral_range: "32768 60999",
    reserved_ports: "1000-1100,8080",
    unprivileged_port_start: "1024",
    ingress_runtime_pid: "",
    ingress_service: "inactive",
    ingress_security: "unavailable",
    ingress_api_b64: "",
    ...Object.fromEntries(
      Object.entries(overrides).filter(
        (entry): entry is [string, string] => entry[1] !== undefined,
      ),
    ),
  });
}
