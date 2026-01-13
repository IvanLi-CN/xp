# Config contracts（#0010）

## `UiThemeConfig`

### Storage

- Storage: `localStorage`
- Key: `xp_ui_theme`

### Values

- `system`：跟随系统（默认建议）
- `light`
- `dark`

### Default policy

- If key missing: default to `system`
- If `system`: derive from `prefers-color-scheme`

### Notes

- Implementation must set the daisyUI theme via `data-theme` on `<html>` or `<body>`.
- Existing auth token storage keys remain unchanged.

## `UiDensityConfig`

### Storage

- Storage: `localStorage`
- Key: `xp_ui_density`

### Values

- `comfortable`（默认建议）
- `compact`

### Scope

- Affects: tables, forms, list spacing, button sizes (within reason).

### Default policy

- If key missing: default to `comfortable`
