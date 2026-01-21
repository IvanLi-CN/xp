# 命令行（CLI）

## `xp-ops tui`

- 范围（Scope）: external
- 变更（Change）: Modify

### 用法（Usage）

```text
xp-ops tui
```

### 交互（Interaction）

说明：以下 key bindings 以 `Help` 面板显示为准。

- 焦点切换：
  - `Tab` / `Shift+Tab`
  - `↑` / `↓`
  - 鼠标左键点击（选择字段/切换编辑项）

- 命令操作（Ctrl 组合键）：
  - `Ctrl+S`: 保存当前配置（写入 deploy settings；按策略处理 token）
  - `Ctrl+D`: **自动保存后**开始 deploy
  - `Ctrl+Q`: 退出（若存在未保存更改则进入保存确认）

保存确认（当存在未保存更改时）：

- 无默认选项：必须显式按键选择
- `Ctrl+S`: 保存并退出
- `Ctrl+Q`: 不保存直接退出
- `Esc` / `Enter`: 取消退出（返回 TUI）

未保存更改（dirty state）判定：

- 任何字段变化均计入（包含 `cloudflare_token` 输入框的变化）

### 输出（Output）

- Format: human（interactive TUI）

### 退出码（Exit codes）

- `0`: 用户正常退出（含保存/不保存）
- `2`: 终端初始化/渲染相关错误（无法进入 TUI）
- `4`: filesystem_error（保存 settings/token 时发生不可恢复错误，且未进入 deploy）

### 兼容性与迁移（Compatibility / migration）

- 本变更不引入新的命令/参数；仅调整 key 行为与退出交互，不影响现有 `xp-ops deploy` CLI。
