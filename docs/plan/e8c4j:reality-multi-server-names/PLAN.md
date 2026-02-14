# VLESS REALITY：多域名 server_names（tags UI）+ Subscription 随机挑选 SNI（#e8c4j）

## 状态

- Status: 待实现
- Created: 2026-02-14
- Last: 2026-02-14

## 背景 / 问题陈述

- 生产环境的部分 VLESS REALITY endpoints 探测失败（canonical target `gstatic-204`），需要一种 **不立即改生产配置**、但能快速支持“多个候选域名 + 逐步验证替换”的能力。
- 当前 Admin UI 仅支持编辑单个 `serverName`（对应 meta 里的 `reality.server_names[0]`），无法表达“候选域名池”。
- Subscription 当前固定输出 `server_names[0]`，无法实现“用户侧随机选一个域名用”的策略。

## 目标 / 非目标

### Goals

- Web Admin：
  - 创建/编辑 VLESS REALITY endpoint 时支持以 **tags/chips** 形式输入多个 `server_names`；
  - 支持设置 primary（`server_names[0]`），并保持 `dest = primary + ":443"` 的派生语义；
  - Endpoint details 的 Configuration 区块可展示全部 server names。
- Backend：
  - Subscription 生成 VLESS REALITY 配置时，从 `server_names` 中 **随机挑 1 个**输出到：
    - Raw URI 的 `sni=...`
    - Clash YAML 的 `servername: ...`
  - 向后兼容：仅配置 1 个 serverName 的 endpoint 行为不变。

### Non-goals

- 不在本计划/PR 内直接修改生产 endpoints 配置（仅提供能力）。
- 不修改 endpoint probe 的行为与数据结构（probe 仍使用 `server_names[0]`）。
- 不在本计划/PR 内保证“大陆直连一定可用”（需要另行在真实网络环境验证域名池）。

## 需求（Requirements）

### MUST

- `server_names` 输入必须支持：
  - `Enter` / `,` 提交 tag；
  - 贴入多行/空格/逗号分隔文本批量解析；
  - 自动 normalize（trim、过滤空、去重保序）；
  - 校验禁止：空白字符、`://`、`/`、`:`、`*`。
- primary 语义固定为 `server_names[0]`：
  - `dest` 始终派生为 `${server_names[0]}:443`；
  - endpoint probe 使用 `server_names[0]`（不在本计划修改）。
- Subscription 输出策略：
  - 每次构建订阅时，对每个 VLESS endpoint 从 `server_names` 随机挑 1 个输出；
  - 当 `server_names=[]` 时保持现有错误（不可输出）。

### SHOULD

- UI 支持“Make primary”快速切换 primary（将某个 tag 移到 index 0）。
- Endpoint details 的 Configuration 区块用 badges 列表展示全部 server names，primary 有明显标记。

## 验收标准（Acceptance Criteria）

- Web：
  - Given 创建 VLESS REALITY endpoint，
    When 输入 2+ 个 server names 并保存，
    Then 刷新页面后仍能正确展示/编辑全部 server names，且 primary 明确。
  - Given 编辑 VLESS REALITY endpoint，
    When 将某个非 primary server name 设为 primary 并保存，
    Then backend 收到的 `dest` 与 `server_names[0]` 一致（`${primary}:443`）。
- Subscription：
  - Given endpoint 的 `server_names` >= 2，
    When 多次请求 subscription（raw/clash），
    Then `sni/servername` 会在候选集合内变化（允许不每次都变化）。
  - Given endpoint 的 `server_names` == 1，
    When 请求 subscription，
    Then 输出与旧行为一致。

## 测试与验证（Testing）

- Rust：`cargo test`
- Web：
  - `cd web && bun run lint`
  - `cd web && bun run typecheck`
  - （建议）`cd web && bun run test`

## 里程碑（Milestones）

- [ ] M1: docs freeze + API payload/compat 明确（本计划文档）
- [ ] M2: Web tags 输入组件 + 新建/编辑页接入
- [ ] M3: Subscription 随机挑选 + Rust/Web tests 更新

