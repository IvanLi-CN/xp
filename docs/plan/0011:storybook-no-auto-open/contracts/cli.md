# CLI Contracts（#0011）

本文件冻结本计划涉及的 CLI 接口口径（内部使用）。

## `web` package scripts

### `bun run storybook`

- Scope: internal
- Change: Modify

**Intent**

- 启动 Storybook dev server（host=`127.0.0.1`，port=`60081`），用于本地 UI 组件调试。
- 默认不产生“自动打开浏览器”的副作用。

**Behavior**

- Starts: Storybook dev server
- Host: `127.0.0.1`
- Port: `60081`
- Browser auto-open: **disabled by default**

**Output**

- Human-readable logs to stdout/stderr
- Must include the accessible URL (e.g. `http://127.0.0.1:60081/`)

**Exit codes**

- `0`: clean exit (manual stop)
- non-zero: start/build failure

### `bun run test-storybook`

- Scope: internal
- Change: None (keep existing behavior)

**Intent**

- 在 CI / 本地对 Storybook 进行自动化检查（Chromium）。

**Behavior**

- Assumes Storybook is reachable at `http://127.0.0.1:60081`
- Runs in CI mode (no browser auto-open; non-interactive)

