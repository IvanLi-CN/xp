# Implementation

## Current State

- The full DaisyUI-to-shadcn/ui migration described in `SPEC.md` is implemented.
- Future UI changes use the maintained component system and Web test coverage.

## Recorded Delivery State

- Status: 已完成
- Created: 2026-03-09
- Last: 2026-03-09

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新规格、索引与技术文档同步到 shadcn/ui 迁移口径。
- [x] M2: `web/` 基础设施完成 Tailwind v4 + shadcn 初始化产物切换，并移除 DaisyUI。
- [x] M3: 共享组件、通用交互与页面表单全部迁到 shadcn/ui / RHF+Zod。
- [x] M4: Storybook stories/docs、自动化测试、浏览器验证与快车道 PR 收敛完成。

## 实施结果（Implementation result）

- `web/` 已移除 DaisyUI 依赖、Tailwind 插件与页面层 Daisy token；样式基线切到 Tailwind CSS v4 + shadcn/ui primitives。
- `UiPrefs` 继续持久化 `xp_ui_theme` / `xp_ui_density`，并同步驱动 `<html data-theme>`、`data-density` 与 `dark` class。
- 登录、用户、节点、配额、服务配置、Reality domains、IP Geo DB 等表单与关键界面已迁到 shadcn wrapper / RHF + Zod。
- Storybook 已覆盖现有通用组件与 `src/components/ui/*` 基础件，Autodocs 为默认 docs 路径。
- 验证通过：`cd web && bun run lint`、`bun run typecheck`、`bun run test`、`bun run build`、`bun run build-storybook`、`bun run test-storybook`、`E2E_BASE_URL=http://127.0.0.1:60180 bun run test:e2e`、`cargo test`。
