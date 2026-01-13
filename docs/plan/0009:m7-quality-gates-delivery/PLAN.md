# Milestone 7 · 质量门禁与交付（#0009）

## 状态

- Status: 已完成
- Created: 2025-12-23
- Last: 2025-12-23

## 背景 / 问题陈述

本计划由旧 planning/spec 文档迁移归档；主人已确认该计划对应功能**已实现**。

## 目标 / 非目标

详见下方“原始输入”中的相关章节（例如“背景与目标”“范围与非目标”等）。

## 范围（Scope）

详见下方“原始输入”。

## 需求（Requirements）

详见下方“原始输入”。

## 接口契约（Interfaces & Contracts）

详见下方“原始输入”（本计划为迁移归档，不在此额外新增契约文档）。

## 验收标准（Acceptance Criteria）

详见下方“原始输入”中的 DoD/验收清单/验收点等章节（如有）。

## 里程碑（Milestones）

- [x] 新增常规 CI 工作流（PR + push main）覆盖：
- [x] 保留并稳定化现有 `xray-e2e` 工作流（Docker + ignored tests）。
- [x] systemd 与 openrc 的启动样例（覆盖 `xp` + `xray`）；
- [x] 最小配置模板（ENV 文件示例），明确必要参数与默认值；
- [x] 数据目录说明（包含 `cluster/`、`raft/`、`state.json`、`usage.json`）；
- [x] 升级/回滚策略（尤其是 schema mismatch 的应对与备份/恢复建议）。

## 方案概述（Approach, high-level）

详见下方“原始输入”。

## 风险与开放问题（Risks & Open Questions）

- None noted in source.

## 参考（References）

- `docs/desgin/README.md`

## 原始输入（迁移前版本）

# Milestone 7 · 质量门禁与交付 — 需求与概要设计

> 对齐计划：`docs/plan/README.md` 的 **Milestone 7**。\
> 参考：`docs/desgin/quality.md` / `docs/desgin/requirements.md` / `docs/desgin/workflows.md`

## 1. 背景与目标

Milestone 1–6 已具备后端控制面、Raft 集群、配额与基础 Web 面板能力；但“能长期稳定交付”的工程门槛仍缺少明确落地：

- CI 尚未覆盖 Rust/Web/Docs 的常规门禁；
- Storybook 的组件回归测试未作为 CI 必跑项；
- 运维交付物缺失：systemd/openrc 示例、最小配置模板、升级/回滚与数据目录说明不完整。

Milestone 7 的目标是交付一套 **可复现、可持续、可运维** 的质量门禁与交付材料：

- CI 与本地门禁对齐：所有关键检查在 PR/主干 push 中可稳定复现；
- 前端回归：Storybook stories 可在 CI 中稳定渲染并执行测试；
- 运维交付：提供 systemd/openrc 样例、最小配置模板、升级/回滚与数据目录说明；
- 保持不引入额外“产品功能”范围，专注于交付与工程质量。

## 2. 范围与非目标

### 2.1 范围（M7 交付）

**CI（GitHub Actions）**

- 新增常规 CI 工作流（PR + push main）覆盖：
  - Rust：`cargo fmt --check`、`cargo clippy -- -D warnings`、`cargo test`
  - Web（Bun）：`biome check`、`tsc -b`、`vitest`、Playwright E2E（关键路径，PR 必跑）
  - Storybook（dev server）：启动 Storybook 并运行 `test-storybook`（PR 必跑）
  - Docs：`dprint` check（Markdown 等）
- 保留并稳定化现有 `xray-e2e` 工作流（Docker + ignored tests）。

**运维交付**

- systemd 与 openrc 的启动样例（覆盖 `xp` + `xray`）；
- 最小配置模板（ENV 文件示例），明确必要参数与默认值；
- 数据目录说明（包含 `cluster/`、`raft/`、`state.json`、`usage.json`）；
- 升级/回滚策略（尤其是 schema mismatch 的应对与备份/恢复建议）。

### 2.2 非目标（明确不做）

- 不做发行/打包体系（deb/rpm、Homebrew、Docker 镜像等）；
- 不引入新的监控/指标后端（Prometheus 等），只保证日志与错误可定位；
- 不把“真实 Xray 数据面连通性测试”纳入 CI 必跑（仍保留现有 gRPC e2e）。

## 3. 关键用例 / 交付流程

1. PR 门禁：
   - 开发者提交 PR → CI 自动运行 Rust/Web/Docs/Storybook（必要时并行）→ 未通过则阻止合入。
2. 主干稳定性：
   - push `main` 触发同等门禁，保证主干始终可发布/可回滚。
3. 运维交付：
   - 运维人员按模板准备数据目录与配置 → 使用 systemd/openrc 启动 → 可定位常见错误（如权限、schema mismatch、证书缺失）。

## 4. 交付物清单（建议落地位置）

**CI**

- `.github/workflows/ci.yml`：Rust/Web/Docs/Storybook 常规门禁
- 保留：`.github/workflows/xray-e2e.yml`

**运维**

- `docs/ops/README.md`：运维总览（目录结构、配置、升级/回滚）
- `docs/ops/systemd/xp.service`：systemd unit 示例
- `docs/ops/systemd/xray.service`：systemd unit 示例
- `docs/ops/openrc/xp`：openrc service 示例
- `docs/ops/openrc/xray`：openrc service 示例
- `docs/ops/env/xp.env.example`：最小配置模板（ENV）

> 若主人希望集中在设计文档目录，也可改为 `docs/desgin/ops.md`；但建议独立 `docs/ops/` 以便后续扩展。

## 5. CI 设计（概要）

### 5.1 触发与并行

- 触发：`pull_request`、push `main`
- Jobs 并行：`rust`、`web`、`storybook`、`docs`

### 5.2 Rust job

- 使用 `rust-toolchain.toml` 指定的 toolchain（当前为 `1.91.0`）
- 运行：
  - `cargo fmt --check`
  - `cargo clippy -- -D warnings`
  - `cargo test`

### 5.3 Web job（含 E2E）

- 前端运行时：Bun（见下方“版本 pin”）。
- 安装依赖：`cd web && bun install --frozen-lockfile`
- 运行：
  - `bun run lint`（Biome）
  - `bun run typecheck`（tsc -b）
  - `bun run test`（vitest）
  - `cd web && bunx --no-install playwright install --with-deps chromium`（CI 预装浏览器）
  - `bun run test:e2e`（PR 必跑）

### 5.4 Storybook job（组件回归）

目标：确保 stories “可渲染、关键交互不报错”，并在 CI 中稳定复现。

- 采用 **dev server + test-storybook**（PR 必跑）：
  - `cd web && bun run storybook`（固定 host/port，建议 `127.0.0.1:60081`）
  - 等待 URL 就绪后运行：`cd web && bunx --no-install test-storybook --url http://127.0.0.1:60081 --ci --browsers chromium --failOnConsole`

> 实现注意：需要确保 `web/package.json` 的 `test-storybook` 脚本真实调用 `test-storybook` CLI（而不是 `storybook test`），否则会导致门禁失效。

### 5.5 Docs job

- `bun install --frozen-lockfile`（repo root）
- `bunx --no-install dprint check`（或 fmt 后检查无 diff）

### 5.6 版本 pin（建议）

- Bun：在仓库根目录新增 `.bun-version` 并固定版本（当前固定为 `1.3.5`）；CI 使用 `oven-sh/setup-bun` 读取并安装；
- Lockfile：仓库根与 `web/` 均使用 `bun.lock`；CI 使用 `bun install --frozen-lockfile` 保证可复现；
- Playwright：依赖版本与 browser install 需配套，避免“本地可跑、CI 不可跑”。

## 6. 运维交付设计（概要）

### 6.1 数据目录结构（对外说明）

以 `XP_DATA_DIR` 指定的数据目录为根：

- `cluster/`：集群元数据与证书材料（`metadata.json`、`cluster_ca.pem`、`node_cert.pem` 等）
- `raft/`：WAL 与 snapshot（强一致状态）
- `state.json`：期望状态（Nodes/Endpoints/Users/Grants）
- `usage.json`：用量与 quota 标记（不进 Raft）

### 6.2 最小配置模板（ENV）

重点说明以下配置的用途与安全边界：

- `XP_DATA_DIR`：数据目录（建议持久化路径）
- `XP_ADMIN_TOKEN`：管理员 token（建议通过 env-file 或 secrets 注入）
- `XP_XRAY_API_ADDR`：本机 Xray gRPC 地址（默认 `127.0.0.1:10085`）

### 6.3 systemd/openrc 启动样例

原则：

- 默认以非特权用户运行 `xp`；
- 明确 `WorkingDirectory` 与 `XP_DATA_DIR` 权限；
- `xp` 与 `xray` 的依赖关系以“参考建议”表达（不强绑定，避免过度假设）。

### 6.4 升级与回滚

最小策略：

- 升级前备份 `XP_DATA_DIR`（尤其是 `cluster/` 与 `raft/`）；
- 若出现 schema mismatch / 启动失败：回滚二进制并恢复备份；
- 集群升级：建议分批滚动，且确保 leader/多数派可用（具体细节后续在运维文档补充）。

## 7. 验收点（DoD 摘要）

- CI：PR/主干 push 必跑，并覆盖 Rust/Web/Docs/Storybook 质量门禁；
- Storybook：stories 回归测试在 CI 中真实执行（可人为引入失败验证门禁有效）；
- 运维：提供 systemd/openrc 示例 + 最小配置模板 + 数据目录说明 + 升级/回滚说明；
- 现有 `xray-e2e` 工作流继续可用且不被常规 CI 误伤。

## 8. 风险点与待确认问题

1. Lockfile 约束：仓库已迁移为 `bun.lock` 并删除 `package-lock.json`；需约定禁止回引 npm lockfile，避免双锁漂移。
2. CI 资源消耗：Storybook + E2E 均为 PR 必跑，需要控制 job 并行与缓存策略，避免 PR 周期过长。
3. 运维模板的路径约定：systemd/openrc 的安装路径与运行用户是否需要更严格约束。
