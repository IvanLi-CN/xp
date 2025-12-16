# xp · 代码质量保证方案（MVP）

> 目标：把质量门槛前置到本地 hooks 与 CI；并通过“前端单元测试 + 后端单元测试 + Storybook 回归”降低 UI/逻辑回归风险。

## 1. 总体原则

- **本地优先**：在 pre-commit 就尽可能阻止明显问题进入提交
- **CI 兜底**：所有本地 checks 在 CI 中必须可复现
- **单测优先**：核心逻辑用单元测试覆盖；E2E 只覆盖关键链路
- **UI 可视化回归**：所有组件必须有 stories，避免“组件没法被独立验证”

## 2. 前端质量门禁（web/）

### 2.1 Biome（格式化 + lint）

- 使用 Biome 完全替代 ESLint/Prettier
- pre-commit 建议对 staged 文件执行并自动修复：
  - `npx @biomejs/biome check --write --staged --files-ignore-unknown=true --no-errors-on-unmatched`
- 规则要求：
  - 禁止未使用变量（由 Biome/TS 配合）
  - 禁止明显的可疑模式（例如空依赖的 hooks 等）

### 2.2 TypeScript 类型检查

- `npx tsc -b`（project references）
- 要求：CI 中 `tsc -b` 必须 0 error

### 2.3 Zod（运行时数据校验，必须）

目标：避免“TS 类型正确但运行时数据不符合预期”导致的前端崩溃/脏数据渲染。

强制要求：

- 所有来自后端的关键 API 响应必须通过 Zod schema 校验后才能进入业务层
- 表单输入（创建用户/授权/端点等）在提交前必须通过 Zod schema 校验
- schema 定义应集中管理，并使用 `z.infer<>` 推导 TS 类型，避免手写重复类型

最低要求（MVP）：

- API client 层对每个 endpoint 的响应做 `safeParse`/`parse`，失败时统一返回可展示的错误（而不是组件内部崩溃）
- 对核心 schema（Grant/Endpoint/User/Usage/Subscription）提供单元测试用例（见下一节的前端单测）

### 2.4 前端单元测试（必须）

测试目标分层：

- **纯函数/工具**：100% 关键路径覆盖（解析/格式化/订阅渲染相关函数等）
- **组件逻辑**：对关键交互与边界状态进行覆盖

建议选型（实现阶段再落地）：

- 测试运行器：Vitest
- 组件测试：React Testing Library（jsdom）
- TanStack Query 测试约定：为每个测试创建独立 `QueryClient`，并用 `QueryClientProvider` 包裹组件，避免跨用例缓存污染
- TanStack Router 测试约定：使用 memory router（或 router 的测试工具）跑路由级交互

最低要求（MVP）：

- 新增/修改业务逻辑必须有对应单测
- 覆盖率门槛建议先从“关键模块”开始设置（避免一开始因为覆盖率拖慢迭代）

## 3. Storybook（必须，且每个组件都要 stories）

### 3.1 强制要求

- `web/src/components/**` 下每个可复用组件必须有 `*.stories.tsx`
- stories 必须覆盖至少三类状态：
  - default / typical
  - edge（空数据、极长文本、loading）
  - error / disabled（如果组件存在该状态）

### 3.2 Storybook 测试（必须）

目标：在 CI 里证明“所有 stories 可渲染、关键交互不报错”。

建议方案：

- `storybook build` 生成静态站点
- 运行 `@storybook/test-runner`（Playwright）对 stories 做 smoke/interaction 测试

最低要求（MVP）：

- CI 中 stories 全量渲染通过
- 对关键组件（订阅展示、授权列表、配额进度条、端点卡片）补充 interaction tests

Storybook 运行环境约定（实现时落地到 `.storybook/preview` 的 decorators）：

- 提供全局 `QueryClientProvider`
- 提供 Router 上下文（TanStack Router）
- API 交互建议通过 mock（例如 MSW）稳定复现各种状态（loading/empty/error）

> 可选增强：后续再引入视觉回归（截图对比）与无障碍（a11y）检查。

## 4. 后端质量门禁（Rust）

### 4.1 格式化与静态检查

- `cargo fmt`：必须无 diff
- `cargo clippy -- -D warnings`：必须 0 warning

### 4.2 后端单元测试（必须）

目标分层：

- **纯逻辑模块**：周期计算（缺日取当月最后一天）、配额判定、订阅生成、状态机变更等
- **适配层**：Xray Controller 的序列化/反序列化与参数构造（可用模拟/fixture）

要求：

- 新增/修改核心逻辑必须有单测
- 优先覆盖“易出错且高风险”的逻辑：
  - Cycle 计算（ByNode/ByUser，UTC+8 固定）
  - 统计增量计算（计数回退、重启归零）
  - 超限封禁与幂等（重复封禁/解封）

### 4.3 集成测试（可选，后续）

在不引入额外服务的前提下：

- 可用 `xp` 的内部模块做“本地端到端”的集成测试（不启动真实 xray）
- 若要测试真实 gRPC：实现阶段再评估（是否使用 xray 二进制作为 test fixture）

## 5. Git Hooks（lefthook）与提交规范

沿用参考项目的 lefthook/commitlint/dprint 思路，并做两点调整：

- Web lint 改为 Biome
- Storybook 测试默认不放在 pre-commit（较慢），放到 CI（或 pre-push）

建议的 hooks（摘要）：

- pre-commit（并行）：
  - `biome check --write --staged`（stage_fixed）
  - `tsc -b`
  - `dprint fmt`（仅 Markdown，stage_fixed）
  - `cargo fmt`（stage_fixed）
  - `cargo clippy -D warnings`
- commit-msg：commitlint（Conventional Commits，英文）

## 6. CI 建议流水线（摘要）

1. Web：
   - `biome check`（不写入）
   - `tsc -b`
   - `vitest`
   - `storybook build` + `test-runner`
2. Rust：
   - `cargo fmt --check`
   - `cargo clippy -- -D warnings`
   - `cargo test`
3. Docs：
   - `dprint check`（或 fmt 后检查无 diff）
