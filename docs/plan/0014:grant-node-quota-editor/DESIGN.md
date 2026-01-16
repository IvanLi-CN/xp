# UI 设计：Grant 按节点配额编辑（0014）

本设计文档描述 User details 页面中的“Access & quota”高保真方案。核心口径：**流量配额按节点（Node）配置，与协议无关**。

## 1) 页面入口与信息架构

- 入口：`/users/$userId`（User details）
- 区块：新增卡片 **Access & quota**
  - 协议接入控制：仍使用“节点 × 协议”矩阵（复用 0012 的 checkbox + endpoint chooser）
  - 节点配额：位于 **节点行头**（每个 node 1 个 quota 值）

“只保留这一种流量设置方式”：

- User details 的节点行头是唯一编辑入口
- 其他页面（Grant details / Grant new 等）仅展示与引导跳转，不再提供第二套配额输入控件

## 2) 布局（Layout）

卡片顶部（Action bar）：

- 左：节点过滤输入（Filter nodes…）
- 中：统计 chip（Selected x / y）
- 右：Reset / Save changes（沿用 0012）
- 次行提示（help line）：
  - `Node quota input: MiB/GiB (default MiB). GB/MB treated as GiB/MiB.`
  - `Quota applies to the node across protocols.`

表格结构：

- 行：Node（node_name + node_id）
- 列：Protocol（VLESS / SS2022 …）
- sticky：第一列（Node）与表头（Protocol header）
- 单元格：仅负责“是否允许该协议接入”与 endpoint 信息/选择

## 3) 节点行头（Node header）结构

每个节点行头包含：

- Node label（加粗）
- Node id（mono、弱化）
- Quota 区：
  - 展示态：`Quota: 10 GiB` + 编辑图标
  - 编辑态：输入框 + Apply/Cancel +（可选）错误提示

Mixed 状态（当后端返回“该节点下 grants 配额不一致”）：

- 展示态显示：`Quota: Mixed`
- 点击编辑后，输入框为空并提示“Set a value to unify”

## 4) 交互细节（冻结口径）

### 4.1 进入编辑态

- 点击行头 quota 展示区域进入编辑态
- 自动 focus 到输入框
- 默认输入值：
  - 非 Mixed：使用当前值的紧凑写法（例如 `10GiB` / `512MiB`）
  - Mixed：输入框为空

### 4.2 提交 / 取消

- 提交：Enter 或点击 Apply
- 取消：Esc 或点击卡片外区域（blur cancel）
- 取消不触发写入；回到展示态并保持原值

### 4.3 校验与错误态

- 输入非法（空、负数、无法识别单位、超出 safe integer）：
  - 输入框 `aria-invalid=true`
  - 红色边框 + **浮层错误提示**（popover/tooltip，位于输入框下方）
  - 错误浮层的显示/隐藏不得改变行高或推动布局（避免 layout shift）
  - 浮层与输入框之间保留最小间距（建议 8px）
  - 浮层容器需要明显的内边距（建议约 6px 纵向 / 10px 横向；或等价的高度/宽度设计）
  - 浮层形态避免“胶囊体”：使用小圆角矩形（例如圆角半径 6px）更贴合错误提示的严肃语义
  - 浮层容器尺寸需自适应文本内容（不写死宽度）：
    - 宽度 `fit-content` + 内边距（例如左右 10px）
    - 建议设置 `max-width` 并允许自动换行（长错误文案不溢出）
  - 浮层必须始终显示在输入框下方（不允许翻转到上方）
  - 浮层必须完整显示在页面可视区域内：当输入框靠近可视区域底部导致下方空间不足时，应先自动滚动最近的可滚动容器（或页面）为浮层腾出空间，然后再显示浮层
  - 为避免 scroll/overflow 裁剪，建议使用 portal（渲染到 `document.body`）并以 viewport 作为裁剪基准
  - Apply 禁用或点击无效（不发请求）
  - 配色建议（贴合整体主题、避免“硬红框+纯白底”突兀）：
    - Light：浅红底 + 中等红边框 + 深红字（例如 `bg #fff1f2 / border #fda4af / text #881337`）
    - Dark：深红半暗底 + 玫红边框 + 浅粉字（例如 `bg #2b0f1a / border #fb7185 / text #fecdd3`）
  - 颜色依据：
    - 使用 Tailwind `rose` 色阶作为 error 语义色（与现有 cyan 主色分离，减少“警告信息”与“主按钮/选中态”争抢注意力）。
    - 对比度按 WCAG AA（小字号 ≥ 4.5:1）：
      - Light：`#fff1f2` vs `#881337` ≈ 8.71:1
      - Dark：`#2b0f1a` vs `#fecdd3` ≈ 12.55:1

### 4.4 提交中（saving）

- Apply 后进入 saving：
  - 输入框与 Apply/Cancel 禁用（避免并发）
  - Apply 展示 loading
  - 成功后更新展示态数值

## 5) 单位解析与格式化（与计划一致）

### 5.1 解析（string → bytes）

- 支持单位：`MiB` / `GiB`（及变体），大小写不敏感，允许空格
- 无单位默认 `MiB`
- 允许小数：换算 bytes 后四舍五入
- `MB/GB` 按二进制口径兼容识别为 `MiB/GiB`
- 空字符串非法；`0` 合法（语义：不做配额封禁）
- 超过 `Number.MAX_SAFE_INTEGER` 视为非法并阻止提交

### 5.2 展示（bytes → human）

- 优先 `GiB`，其次 `MiB`
- 展示文本统一带空格：`10 GiB` / `512 MiB`

## 6) 可访问性与键盘操作

- 编辑态支持：
  - `Enter` 提交
  - `Esc` 取消
- 错误提示使用 `aria-describedby` 关联输入框

## 7) 设计稿（Assets）

- Light：`./assets/grant-node-quota-editor-hifi-light.svg`
- Dark：`./assets/grant-node-quota-editor-hifi-dark.svg`
