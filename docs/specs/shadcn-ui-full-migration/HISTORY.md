# History

- The topic originated in legacy Spec `#d4kex`.
- It records the completed UI component-system migration.

## 变更记录（Change log）

- 2026-03-09: 创建规格并冻结 DaisyUI -> shadcn/ui 全量迁移范围、Tailwind v4、RHF+Zod、Storybook docs 硬门禁与快车道交付口径。
- 2026-03-09: 完成 DaisyUI 依赖与 token 清理，落地 shadcn/ui primitives、Tailwind v4、UiPrefs dark class 同步、RHF+Zod 表单统一与 Sonner/AlertDialog/Dialog/Sheet 迁移。
- 2026-03-09: 为通用组件与 `src/components/ui/*` 基础件补齐 Storybook stories/docs，修复 Storybook test-runner 超时门禁与 E2E 选择器漂移，完成 `lint`/`typecheck`/`test`/`build`/`build-storybook`/`test-storybook`/`test:e2e`/`cargo test` 验证。
