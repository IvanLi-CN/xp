# Fixture 数据策略门禁

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，关键演进原因见
> `./HISTORY.md`。

## 背景 / 问题陈述

测试、Storybook 和 demo fixture 可意外携带生产派生的拓扑、遥测或凭据样式信息。质量文档
要求这些文本源仅使用 synthetic 数据，但此前没有可重复执行的本地与 CI 门禁。

## 目标 / 非目标

### Goals

- 用单一、固定版本的 `ast-grep` 静态规则保护声明的 fixture 文本源。
- 让 catalog 成为操作型 synthetic fixture 值的唯一来源，并为 TypeScript 与 Rust 提供窄
  adapter。
- 让 policy、catalog、adapter 与 CI 命令的修改进入显式 code-owner 审查边界。

### Non-goals

- 不将此 gate 描述为通用 DLP、运行时 provenance 或二进制附件扫描器。
- 不修改生产、集群、Cloudflare、Tunnel、DNS、Raft 或部署行为。
- 不在仓库代码中实施 GitHub branch protection 或权限变更。

## 范围（Scope）

### In scope

- 所有 `web/src/**/*.test.{ts,tsx}`、`*.spec.{ts,tsx}`、`*.stories.{ts,tsx}`、
  `web/tests/**/*.{ts,tsx}`、`web/.storybook/mocks/**/*.{ts,tsx}`、
  `web/src/storybook/**/*.{ts,tsx}` 与 `web/src/demo/fixtures.ts` 中的操作型 fixture 字段。
- `tests/**/*.rs`、`src/**/*_tests.rs`、`src/**/tests.rs`、`src/**/tests/**/*.rs`、
  `src/**/test_fixtures.rs`，以及 `src/**/*.rs` 内 `mod ...tests` 单元测试模块中的操作型
  fixture 字段。
- 上述路径以及两种 adapter 的 suppression 检查。
- catalog、两种 adapter、ast-grep 规则/测试、本地命令、CI workflow 和 code-owner 声明。

### Out of scope

- 截图、二进制、任意非结构化附件与 Git 历史。
- 远端 required-check 或 branch-protection 设置。

## 需求（Requirements）

### MUST

- `bun run check:fixture-policy` 必须先运行 `ast-grep test`，再运行错误级 `ast-grep scan`。
- catalog 必须是单一 checked-in JSON 文件；其中仅允许固定的 `.test` 主机名、文档保留
  地址、测试标识、时间戳和指标。
- data-source rule 列出的操作型字段必须使用直接 catalog adapter accessor；内联值、插值、
  拼接、环境、文件、网络、反序列化和任意来源不得成为 fixture 数据输入。
- 受保护路径不得使用 `ast-grep-ignore`。
- catalog、adapter、scanner 版本、规则 scope 与 workflow 必须受 scoped `CODEOWNERS` 保护。

### SHOULD

- 规则错误必须在 GitHub Actions 中提供文件与行定位。
- 规则回归必须包含允许的 catalog 引用与不含真实数据的拒绝反例。

### COULD

- 在仓库形成同类文本 fixture 时扩展显式 scan scope 和 catalog accessor。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 开发者新增 fixture 数据时，先向 catalog 增加符合语法与枚举规则的 synthetic 值，再在
  adapter 暴露零参数 accessor，最后由受保护字段直接调用该 accessor。
- 本地命令和 CI 使用同一 lockfile 安装的 CLI；任一规则命中以非零状态终止。

### Edge cases / errors

- 禁止的 `ast-grep-ignore` 注释本身必须触发错误，不能作为逃逸机制。
- catalog 中未知文本或数值必须触发 JSON rule，防止 editable catalog 自行放宽值域。
- catalog 中已知键的错误 JSON 类型必须触发 JSON rule，防止以变更值类型绕过 allowlist。
- adapter 中环境、文件、网络或输入反序列化必须触发错误。

## 接口契约（Interfaces & Contracts）

None。此主题只新增开发与 CI gate，不修改产品 HTTP、持久化或运行时接口。

## 验收标准（Acceptance Criteria）

- Given 干净 checkout，When 运行 `bun run check:fixture-policy`，Then 规则测试和扫描均通过。
- Given data-source rule 列出的字段中出现未批准域名、地址、标识、遥测或时间值，When 运行
  gate，Then gate 在文件/行定位处失败。
- Given 受保护字段中的动态或外部来源，When 运行 gate，Then gate 失败。
- Given catalog、adapter、policy 或 workflow 的修改，When 提交 PR，Then `fixture-policy`
  workflow 运行，且路径由 code owner 保护。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- `bun run check:fixture-policy`
- `cd web && bun run lint && bun run typecheck && bun run test`
- `cargo test`

### Quality checks

- `bunx --no-install dprint check`
- GitHub workflow YAML 解析检查

## Visual Evidence

PR: none

## Related PRs

- None

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：ast-grep 是静态语法 gate，不提供跨文件运行时 provenance 保证，也不扫描二进制或
  非结构化附件。
- 假设：仓库所有者会在 GitHub 将 `fixture-policy` 设为 required check 并要求 code-owner
  review。

## 参考（References）

- `../../desgin/quality.md`
- `./IMPLEMENTATION.md`
- `./HISTORY.md`
