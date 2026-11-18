import { dnsName, ensure, fields, object, policyYaml } from "./input.ts";

export const defaultPolicy = {
  cloudflare_tunnel: true,
  cloudflare_ddns: true,
  ip_geo_enrichment: true,
  managed_default_vless: true,
  managed_default_ss2022: false,
  direct_docker: false,
  vless_port: "automatic_preflight",
} as const;
export type Feature = Exclude<keyof typeof defaultPolicy, "vless_port">;
export type EffectivePolicy = Record<Feature, boolean> & { public_hostname_zone: string };
export const features = Object.keys(defaultPolicy).filter(
  (key): key is Feature => key !== "vless_port",
);
export function inventory(text: string): string[] {
  ensure(Buffer.byteLength(text) <= 65536, "hosts inventory exceeds limit");
  const names = text
    .split("\n")
    .map((line) => line.split("#", 1)[0].trim())
    .filter(Boolean);
  ensure(
    names.length > 0 &&
      new Set(names).size === names.length &&
      names.every((name) => /^[a-z0-9][a-z0-9-]*$/.test(name)),
    "invalid or ambiguous hosts inventory",
  );
  return names;
}
export function policy(text: string, hosts: string[], host?: string): EffectivePolicy {
  if (host !== undefined) ensure(hosts.includes(host), "host is not registered");
  const data = policyYaml(text);
  fields(
    data,
    ["schema_version", "owner_confirmed", "geo", "public_hostname", "defaults", "node_overrides"],
    "policy root",
  );
  ensure(
    data.schema_version === 1 && data.owner_confirmed === true,
    "policy schema or owner confirmation is invalid",
  );
  const geo = object(data.geo, "geo");
  fields(geo, ["enabled", "origin", "owner_authorized_third_party_lookup"], "geo");
  ensure(
    geo.enabled === true &&
      geo.origin === "https://api.country.is" &&
      geo.owner_authorized_third_party_lookup === true,
    "geo policy requires explicit authorization and matching origin",
  );
  const publicHostname = object(data.public_hostname, "public_hostname");
  fields(publicHostname, ["zone"], "public_hostname");
  ensure(
    typeof publicHostname.zone === "string" && dnsName(publicHostname.zone),
    "invalid public hostname zone",
  );
  const defaults = object(data.defaults, "defaults");
  fields(defaults, Object.keys(defaultPolicy), "defaults");
  for (const [key, value] of Object.entries(defaultPolicy))
    ensure(defaults[key] === value, `invalid defaults.${key}`);
  const overrides = object(data.node_overrides, "node_overrides");
  let selected: Partial<Record<Feature, boolean>> = {};
  for (const [name, raw] of Object.entries(overrides)) {
    ensure(hosts.includes(name), "override is not registered");
    const override = object(raw, "override");
    fields(override, ["owner_approved", "reason", "defaults"], "override");
    ensure(
      override.owner_approved === true &&
        typeof override.reason === "string" &&
        override.reason.trim(),
      "override requires owner approval and reason",
    );
    const values = object(override.defaults ?? {}, "override defaults");
    fields(values, features, "override defaults");
    for (const value of Object.values(values))
      ensure(typeof value === "boolean", "override defaults must be boolean");
    ensure(
      !(values.cloudflare_tunnel === false && values.managed_default_vless === false) ||
        values.direct_docker === true,
      "private node requires direct_docker",
    );
    if (name === host) selected = values as Partial<Record<Feature, boolean>>;
  }
  return { ...defaultPolicy, ...selected, public_hostname_zone: publicHostname.zone };
}
