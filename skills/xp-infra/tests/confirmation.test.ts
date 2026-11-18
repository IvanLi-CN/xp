import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "bun:test";

const skill = readFileSync(join(import.meta.dir, "../SKILL.md"), "utf8");
const cli = readFileSync(join(import.meta.dir, "../references/cli.md"), "utf8");
const runbook = readFileSync(join(import.meta.dir, "../references/runbook.md"), "utf8");

test("XP-07 host-managed upgrade does not invent an unsupported --yes flag", () => {
  const command = "sudo xp-ops upgrade --version VERSION";
  expect(skill).toContain("不得编造确认参数");
  expect(skill).toContain("检查通过且已有修改授权");
  expect(cli).toContain(`\n${command}\n`);
  expect(cli).toContain("`xp-ops upgrade` does not expose `--yes`");
  expect(cli).not.toContain(`${command} --yes`);
  expect(runbook).toContain("该命令没有 `--yes`");
});
