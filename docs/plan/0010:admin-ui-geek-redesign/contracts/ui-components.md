# UI Component contracts（#0010）

本文件以 TypeScript 形状描述“可复用组件”的 Props/行为约定，便于实现阶段统一改造。

## `Icon`

### Purpose

- 全站唯一图标入口：所有图标必须来自 Iconify。

### Props (shape)

- `name: string`（Iconify 名称，如 `tabler:layout-dashboard`）
- `size?: number | string`
- `className?: string`
- `ariaLabel?: string`（用于无文字按钮）

### Behavior

- If `ariaLabel` provided: render `aria-label`
- Must support inheriting current text color (e.g. `text-base-content`)

### Icon catalog (design baseline)

（以下为本计划高保真设计图中已使用并冻结的图标 key；实现阶段不得替换为其他 icon set 的同名图标。）

- Icon set policy
  - Default (this plan): Iconify `tabler:`
  - Extension: only via an explicit plan update (freeze new icon set + naming rules)

- Navigation
  - Dashboard: `tabler:layout-dashboard`
  - Nodes: `tabler:server`
  - Endpoints: `tabler:plug`
  - Users: `tabler:users`
  - Grants: `tabler:key`

## `PageHeader`

### Props (shape)

- `title: ReactNode`
- `description?: ReactNode`
- `actions?: ReactNode`（右侧主要操作区，例如 New / Refresh / Theme toggle）
- `meta?: ReactNode`（可选：右侧状态徽标区）

### Layout contract

- Title line uses strong hierarchy (e.g. `text-2xl font-semibold`)
- Description uses muted text
- Actions align to the right on desktop and stack on mobile

## `AppShell` (replacing current `AppLayout` structure)

### Props (shape)

- `brand: { name: string; subtitle?: string }`
- `navItems: Array<{ label: string; to: string; icon: string }>`
- `headerStatus?: ReactNode`（health/leader/term/alerts 摘要）
- `children: ReactNode`

### Behavior

- Responsive: sidebar collapses into drawer on small screens
- Active route highlighting must be clear in both themes
- Theme toggle available from the top bar (exact UI can vary, but location fixed)

## `DataTable` (evolution of `ResourceTable`)

### Props (shape)

- `headers: Array<{ key: string; label: ReactNode; align?: 'left'|'center'|'right' }>`
- `children: ReactNode`
- `density?: 'comfortable'|'compact'`（default from `UiDensityConfig`）
- `caption?: ReactNode`（optional helper/summary）

### Behavior

- Provides consistent container (card-like surface), zebra/hover semantics, and horizontal scroll behavior.
- ID/token/url cells default to monospace and allow copy action patterns (via composition).
