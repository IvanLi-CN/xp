---
name: xp
description: 自托管 Xray 集群控制面，采用克制的运维型产品 UI。
colors:
  background: "oklch(98.5% 0.01 205)"
  foreground: "oklch(20% 0.03 240)"
  card: "oklch(100% 0 0)"
  primary: "oklch(63% 0.22 205)"
  primary-foreground: "oklch(98% 0.01 205)"
  secondary: "oklch(93% 0.04 44)"
  secondary-foreground: "oklch(25% 0.02 45)"
  muted: "oklch(95% 0.01 215)"
  muted-foreground: "oklch(45% 0.03 240)"
  accent: "oklch(96% 0.02 205)"
  accent-foreground: "oklch(24% 0.03 240)"
  border: "oklch(89% 0.01 240)"
  input: "oklch(86% 0.01 240)"
  info: "oklch(66% 0.14 230)"
  success: "oklch(69% 0.17 150)"
  warning: "oklch(78% 0.16 85)"
  destructive: "oklch(59% 0.22 25)"
  dark-background: "oklch(18% 0.02 250)"
  dark-foreground: "oklch(95% 0.01 205)"
  dark-card: "oklch(22% 0.02 250)"
  dark-primary: "oklch(70% 0.18 205)"
  dark-border: "oklch(30% 0.02 250)"
typography:
  display:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", system-ui, sans-serif"
    fontSize: "1.5rem"
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: "0"
  headline:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", system-ui, sans-serif"
    fontSize: "1.25rem"
    fontWeight: 600
    lineHeight: 1.25
    letterSpacing: "0"
  title:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", system-ui, sans-serif"
    fontSize: "1.125rem"
    fontWeight: 600
    lineHeight: 1
    letterSpacing: "0"
  body:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", system-ui, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 400
    lineHeight: 1.5
    letterSpacing: "0"
  label:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", system-ui, sans-serif"
    fontSize: "0.75rem"
    fontWeight: 500
    lineHeight: 1.33
    letterSpacing: "0"
  mono:
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, \"Liberation Mono\", monospace"
    fontSize: "0.75rem"
    fontWeight: 500
    lineHeight: 1.4
    letterSpacing: "0"
rounded:
  sm: "0.75rem"
  md: "0.875rem"
  lg: "1rem"
  xl: "1.25rem"
spacing:
  page: "1.5rem"
  page-compact: "1rem"
  card: "1.5rem"
  card-compact: "1rem"
  table-y: "0.75rem"
  table-x: "0.75rem"
  table-compact-y: "0.5rem"
  table-compact-x: "0.625rem"
  field-gap: "0.5rem"
components:
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.primary-foreground}"
    typography: "{typography.body}"
    rounded: "{rounded.sm}"
    padding: "0.5rem 1rem"
    height: "2.5rem"
  button-compact:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.primary-foreground}"
    typography: "{typography.label}"
    rounded: "{rounded.sm}"
    padding: "0.375rem 0.75rem"
    height: "2rem"
  input-default:
    backgroundColor: "{colors.background}"
    textColor: "{colors.foreground}"
    typography: "{typography.body}"
    rounded: "{rounded.sm}"
    padding: "0.5rem 0.75rem"
    height: "2.5rem"
  badge-status:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.accent-foreground}"
    typography: "{typography.label}"
    rounded: "{rounded.xl}"
    padding: "0.125rem 0.625rem"
  table-shell:
    backgroundColor: "{colors.card}"
    textColor: "{colors.foreground}"
    typography: "{typography.body}"
    rounded: "{rounded.lg}"
---

# Design System: xp

## Overview

**创意北极星：「安静的控制室」**

xp 应该像一个紧凑的控制室，服务一位谨慎的维护者：状态始终可见，操作靠近被操作资源，集群异常时界面仍保持镇定。这个系统使用熟悉的产品 UI 模式，因为用户来到这里是为了管理 nodes、endpoints、users、quotas、subscriptions 和 runtime configuration，而不是欣赏品牌页面。

具体使用场景是：自托管维护者在维护窗口里用笔记本或 27 英寸显示器检查集群，有时是在深夜，旁边开着日志，容忍视觉噪音的空间很小。这个场景同时支持 `xp-light` 的日常工作和 `xp-dark` 的暗光维护；两套主题都不是装饰。

视觉系统拒绝赛博扮相、霓虹黑客感、终端雨、电路装饰、hero metrics、装饰渐变和重复的同质卡片网格。它偏向高密度表格、克制状态徽标、清晰文案，以及由 Tailwind CSS v4、shadcn/ui、Radix primitives、Sonner 和 Iconify Tabler icons 组成的稳定 app shell。

文档正文默认使用中文，便于维护者快速扫读；章节标题、design token、component、route、protocol 和运维对象名可以保持英文，以便和 Stitch、代码、Storybook、浏览器调试和后端字段对齐。

**关键特征：**

- 克制的运维色彩，cyan 只作为少量动作和选中强调。
- 原生感系统字体，服务紧凑标签和机器值。
- 低阴影、细边框、色调分层，圆角克制而非可爱。
- 通过 `xp_ui_theme` 持久化 `system`、`light`、`dark` 主题偏好。
- 通过 `xp_ui_density` 持久化 `comfortable`、`compact` 密度偏好。

## Colors

调色板以冷静的运维中性色为基础，只有一个饱和 cyan 作为动作声音，amber 仅用于打破单一蓝灰感，语义色只用于状态。

### Primary

- **集群 Cyan** (`oklch(63% 0.22 205)`): 用于主操作、当前导航、选中状态、焦点环和关键链接。它必须稀缺；accent 应该标记当前路径或动作，而不是装饰静态内容。
- **夜间 Cyan** (`oklch(70% 0.18 205)`): 深色主题主色。它需要足够明亮以识别动作，但不能变成霓虹。

### Secondary

- **温暖配置面** (`oklch(93% 0.04 44)`): 用于次级徽标、中性强调和低风险分组，让产品不要读成单一蓝灰系统。
- **深色工具面** (`oklch(29% 0.03 250)`): 深色主题下的 sidebar、menu 和分组控件表面。

### Tertiary

- **信息蓝** (`oklch(66% 0.14 230)`): 信息徽标和后端健康状态。
- **配额绿** (`oklch(69% 0.17 150)`): 成功状态和健康配额状态。
- **周期 Amber** (`oklch(78% 0.16 85)`): 警告、partial cluster responses，以及有风险但可恢复的状态。
- **故障红** (`oklch(59% 0.22 25)`): 危险操作、失败请求，以及连接或策略已破坏的状态。

### Neutral

- **浅控制面** (`oklch(98.5% 0.01 205)`): 浅色主题页面背景。
- **墨色 Slate** (`oklch(20% 0.03 240)`): 浅色主题正文和高重要性标签。
- **面板白** (`oklch(100% 0 0)`): 当前 card 和 popover surface。除非替换现有 token，不要扩展出新的纯白层级。
- **柔和蓝灰** (`oklch(95% 0.01 215)`): muted rows、subtle panels 和安静的加载骨架状态。
- **发丝边框** (`oklch(89% 0.01 240)`): 分隔线、表格外框、输入边框和面板描边。
- **深控制面** (`oklch(18% 0.02 250)`): 深色主题页面背景。
- **夜间面板** (`oklch(22% 0.02 250)`): 深色主题 card 和 popover surface。
- **夜间发丝线** (`oklch(30% 0.02 250)`): 深色主题分隔线和面板描边。

### Named Rules

**单一动作声音规则。** Primary cyan 只用于动作、选中、链接和焦点。不要把它撒在静态卡片、图标或装饰上。

**状态色规则。** Info、success、warning、destructive 是语义色。不要把它们当品牌装饰色使用。

## Typography

**Display Font:** system sans (`-apple-system`, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif)\
**Body Font:** system sans (`-apple-system`, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif)\
**Label/Mono Font:** 标签使用 system sans；ID、token、URL、端口和配额值使用 `ui-monospace`, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace。

**性格：** 字体系统应当原生、高效、精确。层级依靠字重、对齐和间距，而不是展示字体或超大标题。

### Hierarchy

- **Display** (600, `1.5rem`, `1.2`): 仅用于页面标题和最重要的 shell 级标题。
- **Headline** (600, `1.25rem`, `1.25`): dashboard、详情页和配置页中的 section heading。
- **Title** (600, `1.125rem`, `1`): card title、表格分组标签和 dialog title。
- **Body** (400, `0.875rem`, `1.5`): 主要界面文案、说明、表格正文和表单帮助。解释性长文本控制在 65 到 75ch；数据表可以更宽。
- **Label** (500, `0.75rem`, `1.33`): 徽标、紧凑按钮、表头、字段标签和元数据。
- **Mono** (500, `0.75rem`, `1.4`): 机器值。必须配合一致换行和复制入口，让值可检查。

### Named Rules

**产品字体规则。** 不要为标签、按钮、表格、数据或导航引入展示字体。调校好的单一系统字体就是产品声音。

## Elevation

xp 使用细边框、色调分层和极轻阴影的混合策略。深度用于识别表面和交互状态，不能让管理界面像一叠漂浮卡片。大多数 panel 静止时是 flat，以 `border-border/60` 或 `border-border/70` 建立边界；阴影只保留 `shadow-xs` 或 `shadow-sm`。

### Shadow Vocabulary

- **发丝抬升** (`box-shadow: var(--tw-shadow-xs)` through Tailwind `shadow-xs`): 用于 input、keyboard hint 和需要轻微触感分离的低风险控件。
- **面板抬升** (`box-shadow: var(--tw-shadow-sm)` through Tailwind `shadow-sm`): 用于 card、table shell 和 primary button 的静止状态。
- **焦点环** (`box-shadow: 0 0 0 3px color-mix(in oklab, var(--ring) 20%, transparent)` as Tailwind `focus-visible:ring-[3px] focus-visible:ring-ring/20`): 用于键盘和校验焦点。这是交互反馈，不是装饰光晕。

### Named Rules

**Flat First 规则。** 如果一个表面能用边框和色调背景解释，就先这样做，再考虑 elevation。

## Components

组件应该精确、可重复。它们基于 shadcn/ui primitives、app-level wrappers，并通过 `Icon` 组件使用 Iconify Tabler icons。

### Buttons

- **Shape:** 默认按钮使用 `rounded-xl`，紧凑按钮使用 `rounded-lg`。
- **Primary:** 使用 `bg-primary text-primary-foreground`、`h-10 px-4 py-2`、`text-sm font-medium` 和 `shadow-sm`。
- **Hover / Focus:** hover 将背景调整到 `primary/90`；focus 使用共享 3px ring。disabled 状态使用透明度并阻止 pointer events。
- **Secondary / Ghost / Danger:** secondary 使用 outline 或中性表面，ghost 只在 hover 时使用 accent，danger 使用 `bg-destructive text-destructive-foreground`。
- **Icons:** 按钮可包含 Tabler icons；仅图标按钮如果含义不够显而易见，必须有 accessible label 或 tooltip。

### Chips

- **Style:** 状态 chip 使用 full rounded、紧凑尺寸和低透明语义背景，例如 `info/14`、`success/14`、`warning/18`、`destructive/14`。
- **State:** chip 用于状态和紧凑元数据，不用于装饰。Health、leader、term、alerts、quota 和 endpoint status 应该能快速扫读，并在页面间对齐。

### Cards / Containers

- **Corner Style:** 当前 card 和 panel 使用 `rounded-2xl`，由 root radius scale 支撑。
- **Background:** 主要 panel 使用 `bg-card`，安静 panel 使用 `bg-muted/35`，嵌套 control surface 使用 `bg-background`。
- **Shadow Strategy:** 只有当表面需要从页面背景中分离时才使用 `shadow-sm`。
- **Border:** 使用 `border border-border/60` 或 `border-border/70`。不要使用粗侧边条 accent。
- **Internal Padding:** comfortable density 使用 `1.5rem`，compact density 通过 `--xp-card-padding` 使用 `1rem`。

### Inputs / Fields

- **Style:** 使用 `h-10`、`rounded-xl`、`border-input`、`bg-background`、`px-3 py-2` 和 `text-sm`。
- **Focus:** 将边框切到 `ring` 并应用 3px 低透明焦点环。
- **Error / Disabled:** error 在字段附近使用 destructive 语义；disabled 使用透明度和 blocked cursor。API error 返回时保留已提交值。

### Navigation

- **Style:** App shell 使用稳定 sidebar 加顶部状态区。导航分组需要标签，图标来自 `tabler:`，当前路由必须清楚选中。
- **Typography:** 标签保持紧凑可读；route label 应匹配产品名词。
- **States:** hover 使用 accent surface，active 使用 primary 或 accent 并保证对比度，mobile 使用 Sheet 风格 drawer，而不是自定义导航模式。

### Tables

- **Style:** 表格外壳使用 `xp-table-wrap`，具备横向 overflow、圆角、边框和 `shadow-sm`。
- **Density:** comfortable cell padding 为 `0.75rem`；compact cell padding 为 `0.5rem 0.625rem`。
- **Rows:** zebra row 使用 `bg-muted/25`；分隔线使用 `border-border/60`。
- **Data:** ID、URL、token、port 和 quota value 应使用等宽呈现和可预测对齐。

### Command and Dialog Surfaces

- **Command Palette:** Command-K 是专家入口，不是营销特效。结果应按运维目的地分组。
- **Dialogs:** dialog 用于阻断决策、危险确认和需要隔离的聚焦表单。常规编辑优先使用 inline 或 page-level flow。
- **Sheets:** Sheet 用于移动端导航或次级 panel，让用户仍能理解自己处于当前页面上下文中。

## Do's and Don'ts

### Do:

- **Do** 通过 `UiPrefs` 使用 `xp-light` 和 `xp-dark`，并将 `xp_ui_theme` 持久化为 `system`、`light` 或 `dark`。
- **Do** 使用 `xp_ui_density` 驱动 comfortable 和 compact 下的 table、card、form spacing。
- **Do** 保持 primary cyan 稀缺：只用于动作、当前导航、链接、选中和焦点。
- **Do** 通过 `Icon` 组件渲染所有图标，并使用 `tabler:` 名称。
- **Do** 让机器值可检查：使用等宽文本、复制入口和安全换行。
- **Do** 为可复用组件覆盖 loading、empty、error、disabled、focus 和 destructive 状态。

### Don't:

- **Don't** 把 UI 做成赛博黑客玩具：不要霓虹绿终端、代码雨、电路板装饰或伪入侵视觉。
- **Don't** 在已认证产品界面中使用 hero metrics、销售文案、装饰渐变、glass cards 或重复图标卡片网格。
- **Don't** 用大于 `1px` 的 `border-left` 或 `border-right` 作为 card、list item、callout 或 alert 的彩色侧边条。
- **Don't** 使用渐变文字、UI 标签里的展示字体、bounce/elastic motion 或 page-load choreography。
- **Don't** 在现有兼容 token 之外引入新的纯黑或纯白中性色；新增 neutral 应向产品色相轻微着色。
- **Don't** 混用图标库，或绕过 Tabler/Iconify wrapper。
