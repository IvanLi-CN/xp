import { afterEach, describe, expect, test } from "bun:test";
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { preflight } from "../scripts/preflight.ts";

// GNU dd fullblock, /proc token handling and POSIX shells are verified on Linux.
// Fake command adapters replace service/Docker inventory, never the shell checks.
const linuxProbe = process.platform === "linux" ? describe : describe.skip;
const dirs: string[] = [];
afterEach(() => {
  for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});
function fixture(mode: string) {
  const dir = mkdtempSync(join(tmpdir(), "xp-linux-probe-"));
  dirs.push(dir);
  mkdirSync(join(dir, "bin"));
  mkdirSync(join(dir, "proc/99"), { recursive: true });
  writeFileSync(join(dir, "proc/99/cmdline"), "xray\0run\0");
  symlinkSync("/usr/bin/xray", join(dir, "proc/99/exe"));
  writeFileSync(
    join(dir, "proc/99/stat"),
    `99 (xray) ${["S", ...Array(18).fill("0"), "12345", "0"].join(" ")}\n`,
  );
  const tool = (name: string, source: string) => {
    const path = join(dir, "bin", name);
    writeFileSync(path, "#!/bin/sh\nset -eu\n" + source);
    chmodSync(path, 0o755);
  };
  tool("xp-ops", `printf '%s' '{"runtime":"xray","state":"active","error":null}'`);
  tool(
    "curl",
    `printf '%s\\n' curl >> "$XP_FIXTURE/calls"; case " $* " in
 *' --http1.1 '*) printf '204 1.1' ;;
 *' --http2 '*) printf '204 2' ;;
 *) exit 1 ;;
esac`,
  );
  tool(
    "sudo",
    `printf '%s\\n' "sudo $*" >> "$XP_FIXTURE/calls"
[ "$XP_MODE" != deny-sudo ] || exit 1
[ "$XP_MODE" != direct ] || exit 1
[ "$XP_MODE" != container-drift ] || exit 1
[ "$XP_MODE" != token-drift ] || exit 1
shift
case "$1" in
  test) exit 0 ;;
  sh)
    case "$3" in
      *pidof*)
        n=0; [ ! -f "$XP_FIXTURE/pid-count" ] || n=$(cat "$XP_FIXTURE/pid-count")
        n=$((n + 1)); printf %s "$n" > "$XP_FIXTURE/pid-count"
        if [ "$XP_MODE" = pid-drift ] && [ "$n" -gt 1 ]; then printf 124; else printf 123; fi ;;
      *stat_file*)
        n=0; [ ! -f "$XP_FIXTURE/start-count" ] || n=$(cat "$XP_FIXTURE/start-count")
        n=$((n + 1)); printf %s "$n" > "$XP_FIXTURE/start-count"
        if [ "$XP_MODE" = start-drift ] && [ "$n" -gt 1 ]; then printf 457; else printf 456; fi ;;
      *'! -e'*) exit 0 ;;
      *) exit 1 ;;
    esac ;;
  docker) shift; exec "$XP_FIXTURE/bin/docker" "$@" ;;
  *) exit 1 ;;
esac`,
  );
  tool(
    "docker",
    `printf '%s\\n' "docker $*" >> "$XP_FIXTURE/calls"
[ "$1" != --context ] || shift 2
case "$1" in
  inspect)
    case "$3" in
      *'.Id'*)
        n=0; [ ! -f "$XP_FIXTURE/id-count" ] || n=$(cat "$XP_FIXTURE/id-count")
        n=$((n + 1)); printf %s "$n" > "$XP_FIXTURE/id-count"
        if [ "$XP_MODE" = container-drift ] && [ "$n" -gt 2 ]; then
          printf '%064d' 1
        else printf '%s' "$XP_CONTAINER"; fi ;;
      *Running*) printf true ;;
      *StartedAt*) printf 2026-01-01T00:00:00Z ;;
      *) exit 1 ;;
    esac ;;
  exec)
    shift
    if [ "$1" = -e ]; then
      shift 3
      n=0; [ ! -f "$XP_FIXTURE/token-count" ] || n=$(cat "$XP_FIXTURE/token-count")
      n=$((n + 1)); printf %s "$n" > "$XP_FIXTURE/token-count"
      if [ "$XP_MODE" = token-drift ] && [ "$n" -gt 1 ]; then
        sed 's/12345/12346/' "$XP_FIXTURE/proc/99/stat" > "$XP_FIXTURE/stat-next"
        mv "$XP_FIXTURE/stat-next" "$XP_FIXTURE/proc/99/stat"
      fi
      XP_INFRA_PROCESS_ROOT="$XP_FIXTURE/proc" exec "$@"
    else shift; exec "$@"; fi ;;
  *) exit 1 ;;
esac`,
  );
  return {
    dir,
    env: {
      ...process.env,
      PATH: `${join(dir, "bin")}:/usr/bin:/bin`,
      XP_FIXTURE: dir,
      XP_MODE: mode,
      XP_CONTAINER: "a".repeat(64),
    },
  };
}
async function run(mode: string, container = false, allow = false, runtime = "xray") {
  const f = fixture(mode);
  if (runtime === "xp-proxy" && container) {
    writeFileSync(join(f.dir, "proc/99/cmdline"), "xp-proxy\0run\0");
    rmSync(join(f.dir, "proc/99/exe"));
    symlinkSync("/usr/bin/xp-proxy", join(f.dir, "proc/99/exe"));
  }
  // Relocate only the private config path; execute the retained probe unchanged.
  const source = readFileSync(
    join(import.meta.dir, "../scripts/xp-infra-audit-vless-remote"),
    "utf8",
  ).replace("env_file=/etc/xp/xp.env", `env_file='${f.dir}/xp.env'`);
  const args = container
    ? ["container", "a".repeat(64), "2026-01-01T00:00:00Z", "", "", "99:12345"]
    : ["host", "", "", "123", "456", ""];
  const child = Bun.spawn(
    ["sh", "-s", "--", "fixture-ep.example.test", "4443", String(allow), ...args, runtime],
    { env: f.env, stdin: new Blob([source]), stdout: "pipe", stderr: "pipe" },
  );
  const [out, err, code] = await Promise.all([
    new Response(child.stdout).text(),
    new Response(child.stderr).text(),
    child.exited,
  ]);
  const calls = readFileSync(join(f.dir, "calls"), "utf8");
  return { code, out, err, calls };
}
test("remote probe rejects malformed arguments on every supported host", async () => {
  const source = readFileSync(
    join(import.meta.dir, "../scripts/xp-infra-audit-vless-remote"),
    "utf8",
  );
  const invalidCases = [
    ["fixture-ep.example.test/invalid", "4443", "false", "host", "", "", "123", "456", ""],
    ["fixture-ep.example.test", "4443/tcp", "false", "host", "", "", "123", "456", ""],
    ["fixture-ep.example.test", "4443", "maybe", "host", "", "", "123", "456", ""],
    ["fixture-ep.example.test", "4443", "false", "unknown", "", "", "123", "456", ""],
  ];
  for (const args of invalidCases) {
    const child = Bun.spawn(["sh", "-s", "--", ...args], {
      env: { PATH: "/usr/bin:/bin" },
      stdin: new Blob([source]),
      stdout: "pipe",
      stderr: "pipe",
    });
    const [out, err, code] = await Promise.all([
      new Response(child.stdout).text(),
      new Response(child.stderr).text(),
      child.exited,
    ]);
    expect({ code, out, err }).toEqual({ code: 2, out: "", err: "" });
  }
});
linuxProbe("VER-FOUNDATION-XP-03 isolated Linux probe", () => {
  test("POSIX preflight rejects changing listener snapshots", async () => {
    for (const [drift, container] of [
      [false, false],
      [true, false],
      [false, true],
      [true, true],
    ]) {
      const f = fixture("bootstrap");
      writeFileSync(
        join(f.dir, "bin/sudo"),
        `#!/bin/sh
set -eu
shift
if [ "$1" = sh ] && [ "$2" = -c ]; then
  case "$3" in
    *docker_path=*)
      if [ "$XP_DOCKER" = true ]; then printf %s "$XP_FIXTURE/bin/docker"
      else printf absent; fi; exit 0 ;;
    *'/run/docker.sock'*) printf absent; exit 0 ;;
  esac
fi
exec "$@"
`,
      );
      chmodSync(join(f.dir, "bin/sudo"), 0o755);
      writeFileSync(
        join(f.dir, "bin/ss"),
        `#!/bin/sh
set -eu
n=0; [ ! -f "$XP_FIXTURE/ss-count" ] || n=$(cat "$XP_FIXTURE/ss-count")
n=$((n + 1)); printf %s "$n" > "$XP_FIXTURE/ss-count"
port=22
if [ "$XP_DRIFT" = true ] && [ "$n" -gt 1 ]; then port=23; fi
printf 'LISTEN 0 128 0.0.0.0:%s 0.0.0.0:* users:(("sshd",pid=7,fd=3))\\n' "$port"
`,
      );
      chmodSync(join(f.dir, "bin/ss"), 0o755);
      writeFileSync(
        join(f.dir, "bin/docker"),
        `#!/bin/sh
set -eu
[ -z "\${DOCKER_HOST:-}\${DOCKER_CONTEXT:-}\${DOCKER_CONFIG:-}" ] || exit 1
[ "$1" = --context ] && [ "$2" = default ] || exit 1
shift 2
case "$1" in
  ps) if [ "$2" = --format ]; then
    printf '%s\\n' 'fixture-container ghcr.io/example/xp:fixture'
    else printf fixture-container; fi ;;
  port) : ;;
  inspect)
    case "$3" in
      *'.Id}} {{.State.StartedAt}}'*) printf '%s %s true' "$XP_CONTAINER" 2026-01-01T00:00:00Z ;;
      *Config.User*) printf 1000 ;;
      *CapDrop*|*CapAdd*) printf '[]' ;;
      *) exit 1 ;;
    esac ;;
  exec)
    shift 2
    if [ "$1" = sh ] && [ "$2" = -c ]; then
      case "$3" in
        *XP_DATA_DIR*) printf %s "$XP_FIXTURE/data"; exit 0 ;;
        *XP_XRAY_API_ADDR*) printf 127.0.0.1:10085; exit 0 ;;
        *legacy_path=*) printf '{}'; exit 0 ;;
      esac
    fi
    exec "$@" ;;
  *) exit 1 ;;
esac
`,
      );
      chmodSync(join(f.dir, "bin/docker"), 0o755);
      const source = readFileSync(
        join(import.meta.dir, "../scripts/xp-infra-port-remote"),
        "utf8",
      ).replace("env_file=/etc/xp/xp.env", `env_file='${f.dir}/xp.env'`);
      const child = Bun.spawn(["sh", "-s"], {
        env: {
          ...f.env,
          XP_DRIFT: String(drift),
          XP_DOCKER: String(container),
          DOCKER_HOST: "tcp://fixture.invalid:2375",
          DOCKER_CONTEXT: "untrusted-fixture",
          DOCKER_CONFIG: "/nonexistent-fixture",
        },
        stdin: new Blob([source]),
        stdout: "pipe",
        stderr: "pipe",
      });
      const [out, err, code] = await Promise.all([
        new Response(child.stdout).text(),
        new Response(child.stderr).text(),
        child.exited,
      ]);
      if (drift) expect(code).not.toBe(0);
      else {
        expect({ code, out, err }).toMatchObject({ code: 0 });
        expect(preflight(out, "fixture-node")).toBeGreaterThan(0);
        expect(out).toContain("occupied_ports=22");
      }
    }
  }, 20000);
  test("xp-proxy host and container require both HTTP versions and stable identity", async () => {
    for (const container of [false, true]) {
      const result = await run("host", container, false, "xp-proxy");
      expect({ code: result.code, out: result.out }).toEqual({
        code: 0,
        out: "vless_ingress=true\n",
      });
    }
    expect((await run("pid-drift", false, false, "xp-proxy")).code).not.toBe(0);
    expect((await run("token-drift", true, false, "xp-proxy")).code).not.toBe(0);
  });
  test("host identity is revalidated around successful loopback request", async () => {
    const result = await run("host");
    expect(result.code).toBe(0);
    expect(result.out).toBe("vless_ingress=true\n");
    expect(result.calls).toContain("curl");
  });
  test.each(["pid-drift", "start-drift"])("host identity change fails: %s", async (mode) => {
    const result = await run(mode);
    expect(result.code).not.toBe(0);
    expect(result.out).not.toContain("vless_ingress=true");
  });
  test("host permission failure cannot downgrade to Docker", async () => {
    const result = await run("deny-sudo");
    expect(result.code).not.toBe(0);
    expect(result.calls).not.toContain("docker");
  });
  test("sudo Docker follows process token scan", async () => {
    const result = await run("sudo-container", true);
    expect(result.code).toBe(0);
    expect(result.out).toContain("vless_ingress=true");
    expect(result.calls).toContain("sudo -n docker");
  });
  test("direct Docker needs explicit override", async () => {
    const denied = await run("direct", true, false);
    expect(denied.code).not.toBe(0);
    expect(denied.calls).not.toContain("curl");
    const allowed = await run("direct", true, true);
    expect(allowed.code).toBe(0);
    expect(allowed.out).toContain("vless_ingress=true");
  });
  test.each(["container-drift", "token-drift"])(
    "container identity change fails: %s",
    async (mode) => {
      const result = await run(mode, true, true);
      expect(result.code).not.toBe(0);
      expect(result.out).not.toContain("vless_ingress=true");
    },
  );
  test("runtime environment rejects duplicate keys without secret spooling", async () => {
    const f = fixture("host");
    const envPath = join(f.dir, "proc/99/environ");
    writeFileSync(envPath, "XP_NODE_NAME=fixture\0XP_ADMIN_TOKEN=<SECRET_CANARY>\0OTHER=value\0");
    const source = readFileSync(join(import.meta.dir, "../scripts/xp-infra-audit-remote"), "utf8");
    const body = source.slice(
      source.indexOf("raw_env_payload() {"),
      source.indexOf("\nstrict_bool_value()"),
    );
    const sudo = join(f.dir, "bin/sudo");
    writeFileSync(sudo, '#!/bin/sh\nshift\nexec "$@"\n');
    chmodSync(sudo, 0o755);
    const helper = readFileSync(
      join(import.meta.dir, "../scripts/xp-infra-audit-remote"),
      "utf8",
    ).slice(
      source.indexOf("bounded_output() ("),
      source.indexOf("\ndirect_docker_host_environment_absent()"),
    );
    const script = `set -eu
${body.replace("/proc/$runtime_pid/environ", envPath)}
mode=host
host_xp_pid=99
${helper}
MAX_RAW_BYTES=65536
READ_ERROR_SENTINEL=__READ_FAILED__
raw_env_payload XP_NODE_NAME __END__`;
    async function selected() {
      const child = Bun.spawn(["sh", "-s"], {
        env: f.env,
        stdin: new Blob([script]),
        stdout: "pipe",
        stderr: "pipe",
      });
      const out = await new Response(child.stdout).text();
      const code = await child.exited;
      return { out, code };
    }
    const good = await selected();
    expect(good.code).toBe(0);
    expect(good.out).toBe("fixture__END__");
    expect(good.out).not.toContain("SECRET_CANARY");
    writeFileSync(envPath, "XP_NODE_NAME=fixture\0XP_NODE_NAME=other\0");
    expect((await selected()).out).toContain("__READ_FAILED__");
    expect(source).not.toContain('tr "\\\\000" "\\\\n" < "$env_file" > "$env_dump"');
  });
});

test("runtime environment preserves NUL framing with compatible awk", async () => {
  const f = fixture("busybox-awk");
  const envPath = join(f.dir, "proc/99/environ");
  writeFileSync(envPath, "XP_NODE_NAME=fixture\0XP_ADMIN_TOKEN=<SECRET_CANARY>\0OTHER=value\0");
  const sudo = join(f.dir, "bin/sudo");
  writeFileSync(sudo, '#!/bin/sh\nset -eu\nshift\nexec "$@"\n');
  chmodSync(sudo, 0o755);
  const awk = join(f.dir, "bin/awk");
  writeFileSync(
    awk,
    `#!/bin/sh
set -eu
key=
marker=
program=
while [ "$#" -gt 0 ]; do
  case "$1" in
    -v)
      assignment="$2"
      case "$assignment" in
        key=*) key="\${assignment#key=}" ;;
        marker=*) marker="\${assignment#marker=}" ;;
      esac
      shift 2
      ;;
    *) program="$1"; shift; break ;;
  esac
done
case "$program" in
  *"RS = "*) printf '%s' "$marker" ;;
  *) exec /usr/bin/awk -v key="$key" -v marker="$marker" "$program" "$@" ;;
esac
`,
  );
  chmodSync(awk, 0o755);
  const source = readFileSync(join(import.meta.dir, "../scripts/xp-infra-audit-remote"), "utf8");
  const body = source.slice(
    source.indexOf("raw_env_payload() {"),
    source.indexOf("\nstrict_bool_value()"),
  );
  const helper = source.slice(
    source.indexOf("bounded_output() ("),
    source.indexOf("\ndirect_docker_host_environment_absent()"),
  );
  expect(source).toContain('tr "\\\\n\\\\000" "\\\\001\\\\n" < "$env_file"');
  expect(source).not.toContain('RS = "\\0"');
  expect(source).not.toContain("env_dump");
  const script = `set -eu
${body.replace("/proc/$runtime_pid/environ", envPath)}
mode=host
host_xp_pid=99
${helper}
MAX_RAW_BYTES=65536
READ_ERROR_SENTINEL=__READ_FAILED__
raw_env_payload XP_NODE_NAME __END__
`;
  async function selected() {
    const child = Bun.spawn(["sh", "-s"], {
      env: f.env,
      stdin: new Blob([script]),
      stdout: "pipe",
      stderr: "pipe",
    });
    const [out, err, code] = await Promise.all([
      new Response(child.stdout).text(),
      new Response(child.stderr).text(),
      child.exited,
    ]);
    return { code, out, err };
  }
  const good = await selected();
  expect(good).toEqual({ code: 0, out: "fixture__END__", err: "" });
  expect(good.out).not.toContain("SECRET_CANARY");
  writeFileSync(envPath, "XP_NODE_NAME=fixture\0XP_NODE_NAME=other\0");
  const duplicate = await selected();
  expect(duplicate).toEqual({ code: 0, out: "__READ_FAILED____END__", err: "" });
  writeFileSync(envPath, "OTHER=x\nXP_NODE_NAME=attacker\0XP_NODE_NAME=fixture\0");
  const injected = await selected();
  expect(injected).toEqual({ code: 0, out: "fixture__END__", err: "" });
  writeFileSync(envPath, "XP_NODE_NAME=fixture\nattacker\0");
  const embeddedNewline = await selected();
  expect(embeddedNewline).toEqual({ code: 0, out: "__READ_FAILED____END__", err: "" });
});

linuxProbe("VER-FOUNDATION-XP-03 isolated Linux probe", () => {
  test("full audit collector executes descriptor-bound runtime snapshots", async () => {
    const dir = mkdtempSync(join(tmpdir(), "xp-audit-linux-probe-"));
    dirs.push(dir);
    const dataDir = join(dir, "data");
    mkdirSync(dataDir);
    const envPath = join(dir, "xp.env");
    const procEnvironment = join(dir, "proc-environ");
    const settingsPath = join(dir, "cloudflare-settings.json");
    writeFileSync(
      envPath,
      [
        "XP_BIND=127.0.0.1:62416",
        `XP_DATA_DIR=${dataDir}`,
        "XP_CLOUDFLARE_DDNS_ENABLED=false",
        "XP_CLOUDFLARED_MONITOR_MODE=none",
        "XP_CLOUDFLARED_RESTART_MODE=none",
        "XP_CLOUDFLARE_DDNS_FAST_INTERVAL_SECS=30",
        "XP_CLOUDFLARE_DDNS_INTERVAL_SECS_NO_MONITOR=60",
        "XP_CLOUDFLARE_DDNS_INTERVAL_SECS_WITH_MONITOR=300",
        "XP_CLOUDFLARE_HOSTNAME=",
        "XP_IP_GEO_ENABLED=false",
        "XP_IP_GEO_ORIGIN=https://api.country.is",
        "XP_ENABLE_CLOUDFLARE=false",
        "XP_NODE_NAME=node",
        "XP_API_BASE_URL=https://node.example.test",
        "XP_ACCESS_HOST=node.example.test",
        "XP_XRAY_SYSTEMD_UNIT=xray",
        "XP_XRAY_OPENRC_SERVICE=xray",
        "XP_CLOUDFLARED_SYSTEMD_UNIT=cloudflared",
        "XP_CLOUDFLARED_OPENRC_SERVICE=cloudflared",
      ].join("\n") + "\n",
    );
    writeFileSync(
      procEnvironment,
      "XP_NODE_NAME=node\0XP_API_BASE_URL=https://node.example.test\0" +
        "XP_ACCESS_HOST=node.example.test\0",
    );
    writeFileSync(join(dataDir, "ddns_state.json"), "{}");
    writeFileSync(join(dataDir, "managed-default-endpoints.json"), "{}");
    writeFileSync(settingsPath, "{}");
    mkdirSync(join(dir, "bin"));
    const tool = (name: string, source: string) => {
      const path = join(dir, "bin", name);
      writeFileSync(path, "#!/bin/sh\nset -eu\n" + source);
      chmodSync(path, 0o755);
    };
    tool(
      "sudo",
      `
shift
if [ "$1" = sh ] && [ "$2" = -c ]; then
  case "$3" in
    *pidof*) printf 99; exit 0 ;;
    *stat_file=*) printf 456; exit 0 ;;
  esac
fi
case "$1" in
  xp-ops)
    if [ "\${2:-}" = runtime ]; then
      printf '%s' '{"runtime":"xray","state":"active","error":null}'
    fi
    exit 0 ;;
  *) exec "$@" ;;
esac`,
    );
    tool(
      "docker",
      `
[ "$1" = --context ] && [ "$2" = default ] || exit 1
shift 2
[ "$1" = ps ] && exit 0
exit 1`,
    );
    tool(
      "curl",
      `
url=
for arg in "$@"; do case "$arg" in http://*) url="$arg" ;; esac; done
case "$url" in
  */api/health) printf '{"status":"ok","xray":{"status":"up"}}' ;;
  */api/cluster/info) printf '{"role":"leader"}' ;;
  *) exit 1 ;;
esac`,
    );
    tool("ss", `printf '%s\\n' 'LISTEN 0 128 0.0.0.0:443 0.0.0.0:* users:(("xray",pid=99,fd=3))'`);
    const source = readFileSync(join(import.meta.dir, "../scripts/xp-infra-audit-remote"), "utf8")
      .replace("env_file=/etc/xp/xp.env", `env_file='${envPath}'`)
      .replace("/etc/xp-ops/cloudflare_tunnel/settings.json", settingsPath)
      .replace("/proc/$runtime_pid/environ", procEnvironment);
    async function runCollector() {
      const child = Bun.spawn(["sh", "-s", "--", "false", "true"], {
        env: { ...process.env, PATH: `${join(dir, "bin")}:/usr/bin:/bin` },
        stdin: new Blob([source]),
        stdout: "pipe",
        stderr: "pipe",
      });
      const [out, err, code] = await Promise.all([
        new Response(child.stdout).text(),
        new Response(child.stderr).text(),
        child.exited,
      ]);
      return { code, out, err };
    }
    const result = await runCollector();
    if (result.code !== 0)
      throw new Error(
        `collector failed with code ${result.code}; stdout=${result.out}; stderr=${result.err}`,
      );
    expect(result).toMatchObject({ code: 0, err: "" });
    expect(result.out).toContain("runtime_mode=host\n");
    expect(result.out).toContain("ddns_state_json={}\n");
    expect(result.out).toContain("cloudflare_settings_json={}\n");
    expect(result.out).toContain("managed_default_state_json={}\n");
    expect(result.out).toContain("ingress_public_ports=443\n");
    writeFileSync(
      join(dataDir, "ddns_state.json"),
      Buffer.from('{"snapshot":{"status":"up"}\0}', "binary"),
    );
    const nul = await runCollector();
    expect(nul.code).toBe(1);
    expect(nul.err).toBe("");
    expect(nul.out).not.toContain("ddns_state_json=");
  }, 20000);
});

test("remote JSON preserves whitespace and rejects string newlines", async () => {
  const source = readFileSync(join(import.meta.dir, "../scripts/xp-infra-audit-remote"), "utf8");
  const body = source.slice(
    source.indexOf("compact_json() {"),
    source.indexOf("\nstrict_env_value()"),
  );
  const script = `set -eu
${body}
READ_ERROR_SENTINEL=__READ_FAILED__
value="$1"
emit_value ddns_state_json "$value"
`;
  async function emit(value: string) {
    const child = Bun.spawn(["sh", "-s", "--", value], {
      env: { PATH: "/usr/bin:/bin" },
      stdin: new Blob([script]),
      stdout: "pipe",
      stderr: "pipe",
    });
    const [out, err, code] = await Promise.all([
      new Response(child.stdout).text(),
      new Response(child.stderr).text(),
      child.exited,
    ]);
    return { code, out, err };
  }
  const pretty = await emit(`{
  "snapshot": {
    "status": "up"
  }
}`);
  expect(pretty.code).toBe(0);
  expect(pretty.out).toContain("ddns_state_json={");
  expect(pretty.out).not.toContain("{\n");
  expect(await emit('{"snapshot":{"last_error":"bad\nvalue"}}')).toEqual({
    code: 1,
    out: "",
    err: "",
  });
});

test("runtime state snapshots reject changes after the initial read", async () => {
  const source = readFileSync(join(import.meta.dir, "../scripts/xp-infra-audit-remote"), "utf8");
  const body = source.slice(
    source.indexOf("runtime_file_snapshot() {"),
    source.indexOf("\ncompact_json() {"),
  );
  const script = `set -eu
state='1:10=snapshot-before'
runtime_file_capture() { printf '%s%s' "$state" "$2"; }
${body}
runtime_file_snapshot /var/lib/xp/data/ddns_state.json
expected_identity="$snapshot_identity"
expected="$snapshot_value"
state='2:11=snapshot-after'
runtime_file_snapshot /var/lib/xp/data/ddns_state.json
[ "$snapshot_identity" = "$expected_identity" ]
[ "$snapshot_value" = "$expected" ]
`;
  const child = Bun.spawn(["sh", "-s"], {
    env: { PATH: "/usr/bin:/bin" },
    stdin: new Blob([script]),
    stdout: "pipe",
    stderr: "pipe",
  });
  const [out, err, code] = await Promise.all([
    new Response(child.stdout).text(),
    new Response(child.stderr).text(),
    child.exited,
  ]);
  expect({ code, out, err }).toEqual({ code: 1, out: "", err: "" });
});
