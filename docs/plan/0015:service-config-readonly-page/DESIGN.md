# 服务配置页面高保真设计稿

本文件提供“服务配置”只读页面的高保真视觉规格与交互说明，供实现阶段严格对齐。

## 设计图

- 高保真稿（Light）：`assets/service-config-page.png`
- 视觉源文件（HTML mock）：`assets/service-config-page.html`

## 设计目标

- 让运维“一眼确认”关键运行配置，减少误配与排障时间。
- 保持与现有管理端（DaisyUI + 轻量卡片风格）一致的结构与层级。
- 明确 Settings 分组入口，避免主导航拥挤。

## 页面结构

1. 顶部 Header
   - 复用现有 AppShell 结构：品牌 + 搜索 + 状态徽章。
2. 左侧导航
   - NAV 组（现有模块）
   - SETTINGS 组：新增 “Service config”，高亮当前页。
3. 主内容区
   - Page header：标题 + 描述 + 操作（Copy JSON / Refresh）。
   - Summary chips：状态、节点、刷新时间、access host。
   - 四个配置卡片：Network / Node / Quota / Security。
   - 页脚提示：当前路径与只读语义。

## 信息架构与分组

- **Network**：`bind`、`xray_api_addr`、`api_base_url`
- **Node**：`node_name`、`access_host`、`data_dir`
- **Quota**：`quota_poll_interval_secs`、`quota_auto_unban`
- **Security**：`admin_token_masked`、`admin_token_present`

> `access_host` 为最终字段名，语义为“订阅/客户端连接 host（允许 IP）”。

## 视觉规格（Tokens）

- 与现有主题对齐：使用 `xp-light`（daisyUI winter + 自定义主色）。
- 背景：`base-200` 级别的浅灰蓝（页面底色）。
- 卡片：`base-100` 白底 + 细描边 + 轻阴影。
- 强调色：`primary`（teal，#00a9c7）；告警使用 `secondary`（warm sand，#ff6600）。
- 字体：系统无衬线；值与 token 使用等宽字体。
- 圆角：主容器 `12px`，子卡片 `10px`，字段块 `8px`。

## 组件规格

### Page header

- 标题：`26px`，粗体。
- 描述：`14px`，弱化色。
- 操作按钮：一主一辅（Refresh 为主色）。

### Summary chips

- 4 列（桌面），2 列（中屏）。
- 内容：Label（全大写）+ 值。

### 配置卡片

- 标题 `16px`，描述 `13px`。
- 字段块：label（全大写）+ value（mono）。
- “copy”小标签仅做视觉标识（实际行为见实现阶段）。

## 交互与状态

- **刷新**：点击 Refresh 调用 API 并更新时间戳。
- **Copy JSON**：复制完整配置 JSON（仅非敏感字段）并 toast 提示。
- **Loading**：使用全页 `PageState` loading。
- **Error**：`PageState` error + Retry。
- **Empty/Null**：显示 `(empty)`。

## 文案规范

- 页面标题：`服务配置`
- 描述：`只读展示当前进程配置与订阅入口，便于部署核对与排障。`
- Settings 分组名称：`Settings`
- 页面入口名称：`Service config`

## 可访问性

- 文字与背景对比度 ≥ 4.5:1。
- 所有字段 label 使用大写+间距，便于扫描。

## 与实现对齐的注意点

- `admin_token_masked` 必须是“按实际长度全量打码”。
- `access_host` 允许 IP，空值展示 `(empty)`。
- Copy JSON 应隐藏/脱敏敏感字段（与后端返回一致）。
