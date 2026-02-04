# Node 元数据：配置文件为唯一来源（禁用 UI/API 编辑）（#hmtuc）

## 状态

- Status: 待实现
- Created: 2026-02-04
- Last: 2026-02-04

## 背景 / 问题陈述

当前 `node_name / access_host / api_base_url` 同时存在于：

- 节点本地配置（`xp` 的启动参数 / `/etc/xp/xp.env`）
- 节点本地持久化元数据（`XP_DATA_DIR` 下的 cluster metadata）
- Raft state machine（`Node.*`，用于订阅输出等）
- Raft membership config（`NodeMeta.api_base_url/raft_endpoint`，用于 leader 发现、转发与 Raft RPC）

而 Web 与公开的管理员 API 允许在运行期修改 `Node.api_base_url/access_host/node_name`，但 **不会同步更新 Raft membership 的 NodeMeta**，导致：

- Web 展示与真实可达入口不一致；
- follower -> leader 转发/leader 发现依赖过期 `NodeMeta`；
- 线上域名变更时缺少一条“正规、可审计”的配置路径（应该由 `xp-ops` 通过配置文件管理并应用）。

## 目标 / 非目标

### Goals

- `node_name / access_host / api_base_url` 只能由 `xp-ops` 配置，并落到配置文件中（默认 `/etc/xp/xp.env`）。
- Web UI 不提供上述字段的编辑能力，只读展示即可。
- 公开管理员 API 不提供上述字段的编辑能力（保留 `quota_reset` 管理能力）。
- 提供 `xp-ops` 的正规流程，把配置文件中的节点元数据 **一致性** 应用到：
  - 本节点的 cluster metadata（本地持久化）
  - Raft state machine 的 `Node.*`
  - Raft membership config 的 `NodeMeta`（至少更新本节点；必要时支持批量）

### Non-goals

- 不改变 `quota_reset`：它属于运行期管理动作，应保留 Web 与公开管理员 API。
- 不在本计划内做线上域名迁移；仅提供能力，并在本地 `docker compose` 测试环境验证。

## 范围（Scope）

### In scope

- 后端：移除/禁用对 `Node.node_name/access_host/api_base_url` 的公开管理员 PATCH 能力，仅保留 `quota_reset`。
- 前端：移除 Nodes/NodeDetails 对上述字段的编辑入口；NodeDetails 改为只读展示 +（如有需要）提供 `quota_reset` 编辑入口。
- `xp`：支持从环境变量读取 `XP_NODE_NAME / XP_ACCESS_HOST / XP_API_BASE_URL`（由 `/etc/xp/xp.env` 提供）。
- `xp-ops`：
  - `deploy` 写入上述 env 到 `/etc/xp/xp.env`（或保留已有值，且可覆写）。
  - 新增“应用/同步节点元数据”的命令（只走正规通道），把配置文件作为单一事实来源并同步到集群。
- 测试：在 `scripts/dev/subscription-3node-compose/` 方案中跑回归，验证：
  - UI/API 不再能改 node meta（返回明确错误）
  - 通过 `xp-ops` 同步后，三节点看到的 nodes 列表一致且 leader 发现/转发正常

### Out of scope

- Cloudflare 侧资源创建/修改的直接调用（生产环境仅通过 `xp-ops` 执行）。
- 更改 Raft RPC 的网络拓扑（仍以 `api_base_url` 作为 `raft_endpoint`）。

## 需求（Requirements）

### MUST

- 公开管理员 API：
  - `PATCH /api/admin/nodes/:id` 不允许修改 `node_name/access_host/api_base_url`（明确报错：`invalid_request`）。
  - 仍允许修改 `quota_reset`。
- Web UI：
  - Nodes/NodeDetails 不再提供 `node_name/access_host/api_base_url` 的编辑与保存入口。
- `xp-ops`：
  - `deploy` 能把 `node_name/access_host/api_base_url` 写入 `/etc/xp/xp.env`（以 env 形式保存）。
  - 提供命令将配置文件中的值同步到集群（更新 state machine 与 membership NodeMeta），并具备 dry-run。
- 在本地 docker compose 3 节点环境中，提供一条命令完成：`reset -> up -> seed -> (sync meta) -> verify`。

### SHOULD

- 同步命令输出“将变更什么”的摘要（node_id、旧值、新值）。
- 同步前后做健康检查（leader 可用、`/api/cluster/info` 可读）。

## 验收标准（Acceptance Criteria）

- Given 本地 `scripts/dev/subscription-3node-compose/` 环境启动成功
  When 通过公开管理员 API 尝试修改 node 的 `api_base_url/access_host/node_name`
  Then 返回 `400 invalid_request`（或等价错误），且数据不被更改

- Given 修改某节点的 `/etc/xp/xp.env` 中 `XP_ACCESS_HOST/XP_API_BASE_URL`
  When 运行 `xp-ops` 的同步命令
  Then
  - `GET /api/admin/nodes` 返回的新值一致
  - `GET /api/cluster/info` 的 `leader_api_base_url` 可稳定解析/访问

## 实现里程碑（Milestones）

- [ ] M1: 后端禁用 node meta 的公开编辑（保留 quota_reset）
- [ ] M2: 前端移除 node meta 编辑入口（保留展示；按需补上 quota_reset 编辑）
- [ ] M3: xp/xp-ops 增加“配置文件为单一事实来源 + 同步到集群”的能力
- [ ] M4: docker compose 3 节点环境回归测试脚本覆盖本计划场景

## 风险 / 开放问题

- 风险：Raft membership `SetNodes` 误用可能导致脑裂；需要严格限制仅用于更新既有节点的 NodeMeta（不做成员增删）。
- 开放问题：当 `api_base_url` 变化时，Raft RPC base 与 follower->leader 转发同时依赖它；需要明确“变更窗口”与顺序（先让新入口可达，再同步并滚动重启）。
