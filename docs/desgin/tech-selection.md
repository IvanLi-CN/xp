# xp · 技术选型（MVP）

> 原则：控制面轻量、可维护、跨发行版部署简单；前端以 “Vite + React” 为基础，并用 **Biome.js 完全替代 ESLint**。

## 1. 前端（Web 面板）

参考项目：`IvanLi-CN/codex-vibe-monitor` 的 `web/`（工程结构与 Vite 联调习惯沿用），但质量工具调整为 Biome。

### 1.1 技术栈

- 构建：Vite
- 框架：React
- 路由：TanStack Router（替代 React Router，类型化路由更适合面板类应用）
- 语言：TypeScript（使用 `tsc -b` 做 typecheck）
- 数据校验：Zod（对 API 请求/响应做运行时校验，并可从 schema 推导 TypeScript 类型）
- 数据请求与缓存：TanStack Query（统一管理 server-state：缓存、去重、重试、刷新、失效）
- 样式：Tailwind CSS + DaisyUI（快速出可用 UI）
- E2E：Playwright

### 1.2 代码规范与格式化（Biome）

- Biome：承担 **format + lint**（完全替代 ESLint/Prettier）
- TypeScript 类型检查：仍使用 `tsc -b`（Biome 不承担 TS 类型检查）

### 1.3 Storybook（强制）

- 引入 Storybook 作为组件开发与回归测试载体
- **要求：每个组件必须提供 stories**（包含典型/边界/错误态）
- 配合 `@storybook/test-runner`（Playwright）跑 stories 的“可渲染/交互”测试（见 `docs/desgin/quality.md`）

## 2. 后端（xp，Rust）

参考项目：`codex-vibe-monitor` 的 Rust 依赖取向（Tokio/Axum/Tracing/Clap），并补齐本项目的 Xray gRPC 与 Raft 需求。

### 2.1 推荐依赖方向

- Runtime：Tokio
- HTTP API：Axum
- 中间件：tower-http（trace、cors；静态托管后续决定）
- CLI：Clap derive（`xp init/join/...`）
- 日志：tracing + tracing-subscriber
- 序列化：serde / serde_json（订阅 YAML 另行选择 serde_yaml）
- HTTP Client：reqwest（rustls）
- gRPC：tonic + prost（与本机 Xray gRPC API 交互；也可用于集群内部 RPC）
- 时间/时区：chrono + chrono-tz（ByNode）+ 固定 UTC+8（ByUser）

### 2.2 存储（无外部依赖）

约束解释：不允许外置服务，但允许 `xp` 本地持久化：

- Raft：WAL + snapshot（必须）
- 用量累计：本地 KV/小文件（不进 Raft）

具体嵌入式实现（sqlite/sled/rocksdb/纯文件）留到实现前再评估，以“内存占用/成熟度/维护成本”为准。

## 3. 仓库与构建组织（建议）

采用 monorepo：

- Rust：仓库根（Cargo workspace 或单 crate）
- 前端：`web/`（Vite 项目）
- 文档：`docs/`

开发联调：

- `web` dev server 绑定 `127.0.0.1:60080`
- 通过 Vite proxy 将 `/api`、`/events` 反代到 `xp`（后端默认 `127.0.0.1:<port>`）

## 4. Git hooks 与质量门禁（lefthook）

参考 `codex-vibe-monitor` 的 lefthook 结构，但 Web 改用 Biome：

- pre-commit（并行）：
  - `cd web && npx @biomejs/biome check --write --staged ...`（并 `stage_fixed: true`）
  - `cd web && npx tsc -b`
  - `npx dprint fmt`（仅 Markdown，stage_fixed）
  - `cargo fmt`（stage_fixed）
  - `cargo clippy -- -D warnings`
- commit-msg：commitlint（Conventional Commits，英文）

细节见：`docs/desgin/quality.md`。
