import { afterEach, expect, test } from "bun:test";
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { records } from "../scripts/input.ts";

const dirs: string[] = [];
afterEach(() => {
  for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});
// The bundled default must identify the published XP image, otherwise a
// container node is silently probed as a host and the check is meaningless.
function fixture(image: string, runtimeJson = '{"runtime":"xray","state":"active","error":null}') {
  const dir = mkdtempSync(join(tmpdir(), "xp-boot-"));
  dirs.push(dir);
  mkdirSync(join(dir, "bin"));
  const tool = (name: string, source: string) => {
    writeFileSync(join(dir, "bin", name), `#!/bin/sh\nset -eu\n${source}`);
    chmodSync(join(dir, "bin", name), 0o755);
  };
  tool(
    "docker",
    `case "$1" in
  ps) printf '%s\\n' 'xp-101 ${image}' ;;
  inspect) printf true ;;
  exec) printf '%s' '${runtimeJson}' ;;
  *) exit 1 ;;
esac`,
  );
  // No host sudo on a Docker-operator account.
  tool("sudo", "exit 1");
  return dir;
}
async function probe(dir: string, env: Record<string, string> = {}) {
  const child = Bun.spawn(["sh", join(import.meta.dir, "../scripts/xp-infra-boot-remote")], {
    env: {
      PATH: `${join(dir, "bin")}:/usr/bin:/bin`,
      XP_CONTAINER_IMAGE_PREFIX: "",
      ...env,
    },
    stdout: "pipe",
    stderr: "pipe",
  });
  const [code, out] = await Promise.all([child.exited, new Response(child.stdout).text()]);
  return { code, out };
}
test("the bundled default recognises the published image without extra configuration", async () => {
  const { code, out } = await probe(fixture("ghcr.io/ivanli-cn/xp:3.48.17"));
  expect(code).toBe(0);
  expect(records(out)).toMatchObject({
    runtime: "xray",
    boot_manager: "container",
    boot_consistent: "not_applicable",
  });
});
test("a digest-pinned published image is recognised too", async () => {
  const { code, out } = await probe(fixture("ghcr.io/ivanli-cn/xp@sha256:" + "b".repeat(64)));
  expect(code).toBe(0);
  expect(records(out).boot_manager).toBe("container");
});
test("a foreign image is not treated as an XP container", async () => {
  const { code } = await probe(fixture("ghcr.io/someone-else/xp:3.48.17"));
  expect(code).not.toBe(0);
});
test("an explicit prefix overrides the default", async () => {
  const { code, out } = await probe(fixture("ghcr.io/example/xp:fixture"), {
    XP_CONTAINER_IMAGE_PREFIX: "ghcr.io/example/xp",
  });
  expect(code).toBe(0);
  expect(records(out).boot_manager).toBe("container");
});
