# Subscription 预览格式（#0019）

## 状态

- Status: 待实现
- Created: 2026-01-20
- Last: 2026-01-20

## 背景 / 问题陈述

- Web 管理端的 User Details 页面提供订阅内容预览（base64/raw/clash）与复制能力。
- 现有布局未针对“配置文本（JSON/YAML）= 代码”的阅读方式设计：预览区域过小、自动折行、缺少面向字段复制的快捷操作，导致排障与核对效率很低。
- 需要重新设计订阅区块布局，并把配置预览升级为“代码阅读体验”（不自动折行、可横向滚动、适配 JSON/YAML）。

## 目标 / 非目标

### Goals

- 重新设计 Subscription 区块布局，使订阅内容可读、可核对、可复制（包含长字段）。
- 订阅内容预览默认**不自动折行**，支持横向滚动；配置内容等同代码（code-like）。
- 订阅弹窗采用**左右两栏**：左侧代码视图（全文、搜索），右侧字段面板（关键字段一键复制）。
- 提供面向排障的快捷复制能力（例如复制 `public-key` / `short-id` 等关键字段）。
- 保持复制内容与后端响应一致（copy fidelity），且不破坏既有 Fetch/Reset token 流程与错误提示。

### Non-goals

- 不新增订阅格式，不改变订阅协议语义（raw/base64/clash 的含义与字段不在本计划内重做）。
- 不引入新的鉴权/权限模型（订阅 token 生命周期策略不在本计划范围）。
- 不在本计划内做“大而全”的订阅编辑器（可编辑/自动补全/校验/规则与分组生成等）。

## 用户与场景

- 管理员在 Web 面板进入某个用户详情页：
  - 复制订阅 URL 给客户端；
  - 选择 `raw`/`clash` 并拉取订阅内容，快速检查关键字段（例如 `servername`、`reality-opts.public-key`、`short-id`）；
  - 将内容复制到剪贴板导入客户端或用于排障。

## 需求（Requirements）

### MUST

- Subscription 区块必须调整布局，使“预览区域”相较当前显著增大（至少满足日常排障可读；并提供一键展开到更大视图的能力）。
- 订阅内容预览必须默认不自动折行（不因容器宽度插入换行），并支持横向滚动查看完整行。
- 订阅内容预览必须保留原始文本换行（仅按内容中的 `\n` 换行），并使用等宽字体展示。
- 订阅内容预览必须保留原始缩进与前导空格（不得发生 whitespace collapse），确保 YAML/JSON 的层级结构在“视觉上”与原文一致。
- 预览必须提供语法高亮（syntax highlighting）与行号：`Format=clash` 以 YAML 高亮；当内容可判定为 JSON 时以 JSON 高亮；其他文本按纯文本显示（仍保持不折行 + 横向滚动 + 行号）。
- User details（入口页）不展示订阅内容的 inline preview（避免出现“第二套同功能界面”）；订阅内容只在弹窗中展示。
- User details（入口页）点击 “Fetch” 必须直接打开订阅弹窗并拉取内容（Fetch = open + load）。
- User details（入口页）的 Subscription 卡片在“无 inline preview”的前提下应收敛高度、避免出现大块空白；`Reset token` 应上移到主要信息区（与说明文案同屏可见），不应落在卡片底部制造尴尬空洞。
- 订阅弹窗必须采用左右两栏布局：
  - 左侧：代码视图（全文、不折行、可横向/纵向滚动、搜索/跳转）
  - 右侧：字段面板（至少 `public-key` / `short-id` / `servername`，每项一键复制）
- 左侧代码视图的行号栏必须冻结在左侧（仅随纵向滚动，不参与横向滚动）；横向滚动只作用于代码内容区，行号不得被滚动条遮挡。
- 弹窗头部的操作按钮应成组紧凑排列并右对齐：`Copy URL` / `Copy content` 与 `Close` 相邻，避免出现“悬空在中间”的尴尬留白。
- Search 行布局需避免“右侧空一大块”的观感：Search 输入应尽量占满可用宽度，`Find next/prev` 作为一组右对齐；Search 标签需与输入框在垂直方向居中对齐。
- 字段面板（右栏）内的控件必须有一致的内边距：任何按钮/输入不得贴到面板边缘（左右至少 16px）。
- “Copy content” 必须复制**后端返回的原始文本**（不因预览展示做格式化/折行/重排）。
- 必须提供“关键字段复制”快捷操作：
  - 当 `Format=clash` 且已拉取内容时，至少支持复制 `reality-opts.public-key` 与 `reality-opts.short-id`；
  - 若内容包含 `servername`（或等价字段），应一并提供复制入口。
- 在 compact / normal density 下都应保持可用（不会因为高度/滚动条导致无法选择或阅读）。
- User details 的 Profile 卡片中，`Save changes` 按钮上下必须有可见留白：与上方表单字段至少 16px、与卡片底边至少 16px（不得与输入框区域重叠），避免贴边/压线。
- User details 的 Danger zone 卡片内容（标题 + 说明）在垂直方向应“视觉居中”（上下留白接近），避免出现内容偏上/偏下的失衡观感。

## 接口契约（Interfaces & Contracts）

None（仅调整 Web UI 的展示与交互，不新增/修改后端 API）。

## 验收标准（Acceptance Criteria）

- Given 管理员进入 User Details 页面并选择 `Format=clash`
  When 点击 “Fetch”
  Then 订阅预览弹窗打开并开始拉取内容，预览区域默认不折行，`reality-opts.public-key` 可通过横向滚动完整查看。
- Given 订阅内容已拉取完成
  When 点击 “Copy content”
  Then 剪贴板中的文本与后端响应**字节级一致**（至少验证 `public-key`、`short-id` 等长字段不被插入空格/换行）。
- Given `Format=clash` 的订阅内容已拉取完成
  When 查看代码视图中的 YAML 片段（例如 `proxies:` 下的 `- name` 与其子字段）
  Then 缩进在界面中被完整保留（`- name` 在 `proxies:` 下有缩进，`type/server/port/...` 等字段相对 `- name` 继续缩进，`reality-opts` 下的 `public-key/short-id` 进一步缩进）。
- Given `Format=clash` 的订阅内容已拉取完成
  When 查看代码视图
  Then YAML 语法高亮生效（至少 key / string / number 的颜色区分清晰），并显示行号。
- Given 管理员切换 `Format=raw`
  When 点击 “Fetch”
  Then 弹窗预览区域默认不折行，URI 列表按内容换行显示且单行可横向滚动查看完整 URI。
- Given `Format=clash` 的订阅内容已拉取完成
  When 点击 “Copy public-key / Copy short-id”（或等价快捷入口）
  Then 剪贴板只包含对应字段的完整值（不插入空格/换行）。
- Given 订阅内容已拉取完成且左侧代码区存在横向滚动
  When 将代码内容横向滚动到右侧查看长行
  Then 左侧行号栏仍固定可见、与内容行对齐，且横向滚动条不会覆盖行号栏。
- Given 管理员停留在 User details（入口页）
  When 未打开弹窗
  Then 页面不展示订阅内容的 inline preview（避免重复界面）。
- Given User details 页面渲染完成
  When 查看 Profile 卡片右下角的 `Save changes`
  Then 按钮上下存在可见留白（不贴边），在 compact / normal density 下均一致。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- E2E tests: 扩展 `web/tests/e2e/users-crud.spec.ts`，覆盖：
  - `Format=clash` 的拉取与“默认不折行”展示约束；
  - 关键字段复制（clipboard 内容可断言；需要让 e2e clipboard stub 可读取最后一次写入内容）。
- Storybook: 为 Subscription preview modal 增加/更新 story（含 YAML/JSON 样例），用于人工确认语法高亮、行号冻结、无自动折行与滚动条行为；`cd web && bun run test-storybook` 不得有 console error。

### Quality checks

- 按仓库既有门槛通过：`cd web && bun run lint`、`cd web && bun run typecheck`、`cd web && bun run test`、`cargo test`（如本计划仅改前端，则以 web 侧为主）。

## 文档更新（Docs to Update）

- `docs/desgin/subscription.md`：若最终决定调整 Clash YAML 的序列化/引用策略（例如统一加引号或更换 emitter 行为），需同步更新示例与约束；若仅为 UI 展示优化则无需修改。

## 实现里程碑（Milestones）

- [ ] M1: 重做 Subscription 区块布局（增大预览区域 + 一键展开）
- [ ] M2: 引入/实现代码式预览组件（JSON/YAML/text：不折行 + 横向滚动 + 等宽；语法高亮 + 行号冻结）
- [ ] M3: 增加关键字段复制快捷入口（至少 `public-key` / `short-id` / `servername`）
- [ ] M4: 补齐 E2E 覆盖（raw + clash + clipboard 断言）

## 设计草图（Mock）

（用于确认布局方向与信息密度；实现阶段可按此落地，并在 UI 细节上微调。）

- User details (entry) · Light: [assets/user-details-subscription-entry-light.svg](./assets/user-details-subscription-entry-light.svg)
- User details (entry) · Dark: [assets/user-details-subscription-entry-dark.svg](./assets/user-details-subscription-entry-dark.svg)
- Expanded preview · Light: [assets/user-details-subscription-redesign-light.svg](./assets/user-details-subscription-redesign-light.svg)
- Expanded preview · Dark: [assets/user-details-subscription-redesign-dark.svg](./assets/user-details-subscription-redesign-dark.svg)

## 方案概述（Approach, high-level）

- 优先从前端展示层解决：
  - 将订阅内容展示放入弹窗的“代码视图”（read-only），默认不折行、可横向滚动，提供 YAML/JSON 语法高亮与行号（行号栏冻结）。
  - 复制来源保持为内存中的响应字符串（而非 DOM 渲染结果），确保 copy fidelity。
- 避免出现“两套同功能界面”：
  - User details 页面只保留“关键信息 + 操作入口（Fetch 打开弹窗）”，不展示 inline preview。
  - 大预览（modal）才是完整的代码阅读体验（全文、滚动、行号/搜索等增强），避免在页面内重复堆一个“完整版编辑器”。
- 关键字段复制：
  - `clash`：从 YAML 内容中提取字段（可用轻量解析或受控规则提取），并提供按钮一键复制。
  - `raw/base64`：保持全文复制与不折行预览；字段提取不在本计划强制范围内。
- 后端侧不做变更；若后续发现 YAML 生成本身会插入换行导致语义变化，再单独开计划评估序列化策略。

## 风险与开放问题（Risks & Open Questions）

- 风险：不折行会引入水平滚动，移动端/小屏可读性可能下降，需要权衡默认策略或提供切换。
- 风险：语法高亮与行号通常需要引入代码编辑器/高亮依赖（优先考虑 CodeMirror 6 的 read-only 模式）；需要评估 bundle 体积、样式与 DaisyUI 的一致性，并尽量做到按需加载（modal 打开后再加载）。

## 假设（Assumptions）

- 关键字段复制的最小集合：`reality-opts.public-key`、`reality-opts.short-id`、`servername`（仅针对 `Format=clash` 且已拉取内容时展示）。
- “空间太小”的主要解决方案包含：增大预览区域 + 提供一键展开到更大视图；不引入复杂编辑能力（只读即可）。

## 变更记录（Change log）

- 2026-01-20: 创建计划并记录问题与初步方向
- 2026-01-20: 冻结口径：重做布局、不折行、字段快捷复制，并补充设计草图
- 2026-01-20: 决策：入口页不展示 inline preview；用 Fetch 打开弹窗并拉取内容
- 2026-01-20: 设计修正：避免卡片出现大块空白（Danger zone 收敛高度并补充说明文案）
- 2026-01-20: 设计修正：Profile 的 `Save changes` 按钮上下留白，避免与输入区重叠
- 2026-01-20: 设计修正：弹窗右侧字段面板增加内边距，避免按钮/输入贴边
- 2026-01-20: 设计修正：行号栏冻结在左侧，横向滚动不影响行号可见性
- 2026-01-20: 设计修正：代码视图保留缩进/前导空格（避免 YAML 层级“看起来被打平”）
- 2026-01-20: 需求补充：代码视图启用语法高亮（YAML/JSON）并保留行号
- 2026-01-20: 决策：语法高亮范围为 `clash`=YAML，JSON 可判定时启用 JSON 高亮，其余按纯文本显示
- 2026-01-20: 设计修正：头部 `Copy URL` / `Copy content` 右对齐并与 `Close` 成组，避免按钮悬空
- 2026-01-20: 设计修正：User details 的 Danger zone 文案垂直居中，避免上下留白不均
- 2026-01-20: 设计修正：Search 行标签垂直居中，输入框拉伸占宽并右对齐 `Find next/prev`（消除右侧尴尬空白）
- 2026-01-20: 设计修正：入口页 Subscription 卡片收敛高度并上移 `Reset token`，消除底部大块空白
