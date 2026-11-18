import { readFileSync } from "node:fs";
import { join } from "node:path";
import { spawn } from "node:child_process";
import {
  auditDecisions,
  identity,
  normalizeBasic,
  normalizePublicHealth,
  privateNode,
  type Observation,
} from "./audit.ts";
import { ensure, integer, records } from "./input.ts";
import { cloudflareAddress, probe, publicTarget } from "./network.ts";
import { inventory, policy, type EffectivePolicy } from "./policy.ts";
import { preflight } from "./preflight.ts";
import { command } from "./process.ts";

const scriptDir = import.meta.dir;
const sshOptions = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=10"];
function configuration(): { hosts: string[]; policyFile: string } {
  const env = process.env;
  const config =
    env.XP_INFRA_CONFIG_DIR ||
    (env.XDG_CONFIG_HOME
      ? join(env.XDG_CONFIG_HOME, "xp-infra")
      : env.HOME
        ? join(env.HOME, ".config/xp-infra")
        : "");
  ensure(config || (env.XP_HOSTS_FILE && env.XP_POLICY_FILE), "set XP_INFRA_CONFIG_DIR or HOME");
  return {
    hosts: inventory(readFileSync(env.XP_HOSTS_FILE || join(config, "hosts.txt"), "utf8")),
    policyFile: env.XP_POLICY_FILE || join(config, "deployment-policy.yaml"),
  };
}
async function operator(host: string, hosts: string[]): Promise<Record<string, string>> {
  ensure(hosts.includes(host), "host is not registered");
  const config = await command(["ssh", "-G", host], { timeoutMs: 15000 });
  const values: Record<string, string> = Object.create(null);
  for (const line of config.trim().split("\n")) {
    const [key, value] = line.split(/\s+/);
    if (["user", "hostname", "port"].includes(key)) {
      ensure(!Object.hasOwn(values, key) && value, "ambiguous SSH configuration");
      values[key] = value;
    }
  }
  ensure(
    values.user && !["root", "xp"].includes(values.user),
    "configure a non-root operator account",
  );
  ensure(values.hostname && values.port, "incomplete SSH configuration");
  integer(values.port, 1, 65535, "SSH port");
  return values;
}
async function remote(
  host: string,
  file: string,
  args: string[] = [],
  timeoutMs = 60000,
): Promise<string> {
  ensure(
    args.every((arg) => /^[a-zA-Z0-9:.\-]*$/.test(arg)),
    "unsafe remote probe arguments",
  );
  const prefix = process.env.XP_CONTAINER_IMAGE_PREFIX;
  if (prefix)
    ensure(/^[a-zA-Z0-9][a-zA-Z0-9._/:-]*$/.test(prefix), "invalid container image prefix");
  const input =
    (prefix ? `XP_CONTAINER_IMAGE_PREFIX='${prefix}'\nexport XP_CONTAINER_IMAGE_PREFIX\n` : "") +
    readFileSync(join(scriptDir, file), "utf8");
  return await command(
    ["ssh", ...sshOptions, host, `sh -s -- ${args.map((arg) => `'${arg}'`).join(" ")}`],
    { input, timeoutMs },
  );
}
async function audit(host: string, p: EffectivePolicy): Promise<void> {
  const raw = await remote(host, "xp-infra-audit-remote", [
    String(p.direct_docker),
    String(p.managed_default_vless),
  ]);
  const observed: Observation = normalizeBasic(raw);
  const snapshot = identity(records(raw));
  if (!privateNode(p)) {
    const target = await publicTarget(String(observed.public_health_target), "/api/health");
    const health = await probe(target);
    Object.assign(observed, normalizePublicHealth(health.body, String(observed.ingress_kind)));
    if (p.cloudflare_tunnel) {
      observed.cloudflare_tunnel =
        observed.tunnel_declaration === "aligned" &&
        observed.public_health_json_ok === true &&
        /^server:[ \t]*cloudflare[ \t]*$/im.test(health.headers) &&
        /^cf-ray:/im.test(health.headers) &&
        (await cloudflareAddress(target.address));
    }
  }
  if (p.managed_default_vless) {
    observed.local_canary = false;
    observed.vless_ingress = false;
    if (observed.canary_local_target) {
      const target = await publicTarget(String(observed.canary_local_target), "/generate_204");
      const args =
        snapshot.mode === "host"
          ? ["host", "", "", snapshot.pid, snapshot.starttime, ""]
          : ["container", snapshot.id, snapshot.startedAt, "", "", snapshot.ingressProcessIdentity];
      args.push(String(observed.ingress_kind));
      const local = records(
        await remote(host, "xp-infra-audit-vless-remote", [
          target.host,
          String(target.port),
          String(p.direct_docker),
          ...args,
        ]),
        ["vless_ingress"],
      );
      ensure(["true", "false"].includes(local.vless_ingress), "invalid loopback canary result");
      observed.local_canary = local.vless_ingress === "true";
    }
    if (observed.vless_public_target) {
      const target = await publicTarget(String(observed.vless_public_target), "/generate_204");
      const response = await probe(target);
      observed.vless_ingress = observed.local_canary === true && response.status === 204;
      if (!observed.vless_ingress) observed.ingress_public_listener = false;
    }
  }
  const result = auditDecisions(p, observed);
  for (const line of result.lines) console.log(line);
  ensure(result.ok, `audit: FAILED (${host}); do not declare deployment complete`);
  console.log(`audit: OK (${host})`);
}
const statusProbe = `set -eu
if command -v rc-service >/dev/null 2>&1; then sudo -n rc-service xp status
elif command -v systemctl >/dev/null 2>&1; then sudo -n systemctl status xp --no-pager
else echo 'unknown service manager' >&2; exit 2; fi`;
const versionProbe = `set -eu
if [ -x /usr/local/bin/xp ]; then /usr/local/bin/xp --version
elif command -v xp >/dev/null 2>&1; then xp --version
else exit 1; fi`;
export type Request =
  | { command: "help" | "inventory" | "policy" }
  | { command: "port"; host: string }
  | { command: "audit"; host: string }
  | { command: "status"; host: string }
  | { command: "version"; host: string }
  | { command: "logs"; host: string; tail: number }
  | { command: "tunnel"; host: string; localPort: number; remotePort: number };
export function parseArgs(args: string[]): Request {
  const [cmd = "help", ...rest] = args;
  if (["help", "-h", "--help"].includes(cmd)) {
    ensure(rest.length === 0, "unexpected help arguments");
    return { command: "help" };
  }
  if (cmd === "inventory") {
    ensure(rest.length === 0, "usage: xp-infra inventory");
    return { command: cmd };
  }
  if (cmd === "policy") {
    ensure(rest.length === 1 && rest[0] === "check", "usage: xp-infra policy check");
    return { command: cmd };
  }
  if (cmd === "port") {
    ensure(rest.length === 2 && rest[0] === "preflight", "usage: xp-infra port preflight <host>");
    return { command: cmd, host: rest[1] };
  }
  ensure(
    cmd === "audit" || cmd === "status" || cmd === "logs" || cmd === "version" || cmd === "tunnel",
    "unknown command",
  );
  const host = rest.shift();
  ensure(host && /^[a-z0-9][a-z0-9-]*$/.test(host), "missing or invalid host alias");
  if (cmd === "audit" || cmd === "status" || cmd === "version") {
    ensure(rest.length === 0, "unexpected command arguments");
    return { command: cmd, host };
  }
  if (cmd === "logs") {
    ensure(
      rest.length === 0 || (rest.length === 2 && rest[0] === "--tail"),
      "usage: xp-infra logs <host> [--tail N]",
    );
    return {
      command: cmd,
      host,
      tail: rest.length ? integer(rest[1], 1, 10000, "tail count") : 200,
    };
  }
  let remotePort = 62416,
    localPort: number | undefined;
  const seen = new Set<string>();
  while (rest.length) {
    const flag = rest.shift()!;
    const key = flag === "--ui" || flag === "--metrics" ? "mode" : flag;
    ensure(!seen.has(key), "duplicate tunnel option");
    seen.add(key);
    if (flag === "--ui") remotePort = 62416;
    else if (flag === "--metrics") remotePort = 10085;
    else {
      ensure(flag === "--local-port", "unknown tunnel argument");
      localPort = integer(rest.shift() ?? "", 1, 65535, "local port");
    }
  }
  return { command: cmd, host, remotePort, localPort: localPort ?? remotePort };
}
export async function main(args = process.argv.slice(2)): Promise<void> {
  const request = parseArgs(args);
  if (request.command === "help") {
    console.log(`xp-infra: read-only ops helper (Bun; SSH aliases only)
Usage:
  xp-infra inventory
  xp-infra policy check
  xp-infra port preflight <host>
  xp-infra audit <host>
  xp-infra status <host>
  xp-infra logs <host> [--tail N]
  xp-infra version <host>
  xp-infra tunnel <host> [--ui|--metrics] [--local-port PORT]
Private configuration: XP_INFRA_CONFIG_DIR, XP_HOSTS_FILE, XP_POLICY_FILE.
Transport and public probes require OpenSSH and curl. No Ruby/Python runtime.`);
    return;
  }
  const config = configuration();
  if (request.command === "policy") {
    policy(readFileSync(config.policyFile, "utf8"), config.hosts);
    console.log("policy: OK");
    return;
  }
  if (request.command === "inventory") {
    let failed = false;
    for (const host of config.hosts) {
      console.log(`== ${host} ==`);
      try {
        const ssh = await operator(host, config.hosts);
        console.log(`ssh: user=${ssh.user} host=${ssh.hostname} port=${ssh.port}`);
        const r = records(await remote(host, "xp-infra-inventory-remote"), [
          "os_pretty",
          "svc_mgr",
          "xp_present",
          "xp_version",
          "ui_listen",
          "metrics_listen",
        ]);
        console.log(
          `os: ${r.os_pretty}\nsvc_mgr: ${r.svc_mgr}\nxp_present: ${r.xp_present}\n` +
            `xp_version: ${r.xp_version}\nlisten: ui_62416=${r.ui_listen}` +
            ` metrics_10085=${r.metrics_listen}`,
        );
      } catch {
        console.log("probe: FAILED");
        failed = true;
      }
    }
    ensure(!failed, "inventory contains failed probes");
    return;
  }
  ensure("host" in request, "missing selected alias");
  const host = request.host;
  const p = ["audit", "port"].includes(request.command)
    ? policy(readFileSync(config.policyFile, "utf8"), config.hosts, host)
    : undefined;
  await operator(host, config.hosts);
  if (request.command === "audit") {
    await audit(host, p!);
    return;
  }
  if (request.command === "port") {
    console.log(preflight(await remote(host, "xp-infra-port-remote"), host));
    return;
  }
  if (request.command === "tunnel") {
    const forwarding = await command(["ssh", ...sshOptions, host, "sudo -n sshd -T"], {
      timeoutMs: 15000,
    });
    ensure(
      /^allowtcpforwarding (?:yes|local)$/m.test(forwarding),
      "SSH forwarding is unavailable or unverified",
    );
    console.error(
      `tunnel: localhost:${request.localPort} -> ${host}:127.0.0.1:${request.remotePort}`,
    );
    const child = spawn(
      "ssh",
      [
        "-n",
        ...sshOptions,
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "ServerAliveInterval=30",
        "-o",
        "ServerAliveCountMax=3",
        "-L",
        `127.0.0.1:${request.localPort}:127.0.0.1:${request.remotePort}`,
        host,
        "-N",
      ],
      { stdio: "inherit" },
    );
    await new Promise<void>((resolve, reject) => {
      child.on("error", reject);
      child.on("exit", (code) => (code === 0 ? resolve() : reject(new Error("SSH tunnel failed"))));
    });
    return;
  }
  const script =
    request.command === "status"
      ? statusProbe
      : request.command === "version"
        ? versionProbe
        : `set -eu
if command -v rc-service >/dev/null 2>&1; then
  sudo -n true
  if sudo -n test -f /var/log/xp.log; then sudo -n tail -n ${request.tail} /var/log/xp.log
  else
    curl -fsS --noproxy '*' --max-time 10 --max-filesize 65536 \\
      http://127.0.0.1:62416/api/health
  fi
elif command -v systemctl >/dev/null 2>&1; then
  sudo -n journalctl -u xp --no-pager -n ${request.tail}
else exit 2; fi`;
  process.stdout.write(await command(["ssh", ...sshOptions, host, "sh -s"], { input: script }));
}
export function reportError(error: unknown): void {
  const message = error instanceof Error ? error.message : "operation failed";
  console.error(`error: ${message.replace(/[\x00-\x1f\x7f]/g, " ")}`);
  process.exitCode = 1;
}
