# 全站连接数采样与分层统计

> 当前有效规范以本文为准；实现覆盖见 `./IMPLEMENTATION.md`，主题演进见 `./HISTORY.md`。

## Related ADRs

- [ADR 0036](../../adr/0036-full-socket-connection-classes-and-rollup.md)

## 背景 / 问题陈述

现有 `node-tcp-connection-count` 只在业务 endpoint 监听端口上统计 `ESTABLISHED` 入站
TCP 连接，出站 socket 因本地端口是临时端口而被过滤掉，且记录模型没有方向或归属类别。
管理员既看不到出站连接，也无法区分控制面、托管运行时与其它进程的连接压力。

本主题在同一采样口径上补齐全站分类、分层 rollup 与长期查询。采样值仍然是某一时刻的
瞬时连接数，不是新建连接次数，也不是会话数。

## 目标 / 非目标

### Goals

- 每分钟对当前网络命名空间内的 `ESTABLISHED` TCP 连接做一次有界瞬时采样，并分类记录。
- total 与类别拆分均可查询、可 rollup；解析到的每一行都必须落入一个类别。
- 1m / 5m / daily 分层聚合，local raw 7 天，daily 90 天。
- 通过 Source Delivery Journal 与 History Repository 保存长期历史，不写 Raft。
- 缺口、截断、平台不支持与部分失败有明确质量语义，绝不伪装成零值。
- systemd、OpenRC 与单镜像 Docker/Compose 使用同一合同。

### Non-goals

- 不统计新建连接速率、连接生命周期或严格会话数。
- 不通过 access log 把出站连接归到 endpoint 或远端目标。
- 不做跨节点总览、告警、外部 TSDB、导出或任意标签查询。
- 不延长 1m raw 的 7 天保留；不跨升级回填未观测分钟，仅导入既有 7 天 JSON。
- 不枚举任意进程命令行、不启动 shell 或外部 `ss`/`netstat`。

## 范围（Scope）

### In scope

- Linux 网络命名空间内 IPv4 / IPv6 / loopback 的 `ESTABLISHED` TCP 有界分类采样
  （16,384 行上限）。
- 分钟瞬时样本、本地二进制历史、5m / daily Rollup、专用 History Stream 与保留。
- 现有 node TCP connection API 的类别、窗口、分辨率与质量扩展。
- Node Details `TCP connections` tab 的总线与类别拆分展示。
- endpoint tombstone、node 删除与 snapshot install 的可见视图清理。

### Out of scope

- UDP、UNIX socket、`SYN_*`、`TIME_WAIT`、`CLOSE_WAIT`。
- 非 Linux 节点的主动兼容实现。
- 逐连接五元组持久化与任意远端目标聚合。

## 术语与口径

| 优先级 | 条件                                    | 类别               |
| -----: | --------------------------------------- | ------------------ |
|      1 | `local_port` 属于业务 endpoint 监听端口 | `inbound`          |
|      2 | 匹配控制面监听项（按方向取本端/远端）   | `control_plane`    |
|      3 | socket UID 属于当前 ingress runtime     | `outbound_runtime` |
|      4 | 其余                                    | `outbound_other`   |

- 主体：`inbound` 为 `endpoint_id`；`control_plane` 为 `xp_api` / `xray_api` / `reverse`；
  `outbound_runtime` 为 runtime；`outbound_other` 为 socket UID。
- 控制面集合来自当前配置的监听项：XP API、Xray API、Reverse `10086`。
- wildcard 监听地址（`0.0.0.0` / `::`）匹配任意本端地址；已接受 socket 匹配 local 侧，
  发起 socket 匹配 remote 侧，双侧命中只计一次。
- 分类按优先级命中即止；四类互斥，没有丢弃分支。
- 未截断且四类均已观测时，`total(t)` 恒等于同一采样过程四类计数之和；`truncated` 或
  `classes_observed` 不完整时它是下界，按 `partial` 降级。
- 同一转发会话产生的入站与出站是两个 socket，各计一次。
- 范围是当前网络命名空间；容器节点只看到容器内的 socket，这是部署边界，不是过滤。
- runtime 身份：host-managed 取固定服务 UID；容器取入口固定身份文件中的 PID 集合与
  socket inode。身份不可解析时该节点降级为 `outbound_other` 并标 `runtime_identity_unresolved`。

## 需求（Requirements）

### MUST

#### 采样

- 以 UTC 分钟为桶，每分钟最多持久化一次；写入失败可在同一分钟重试并覆盖，成功后跳过该分钟。
- 顺序读取 `/proc/net/tcp` 与 `/proc/net/tcp6`，只接受状态 `01`，解析 UID 与两侧地址端口。
- 两个文件按固定顺序读取，单次采样跨度有界；`total` 是该次解析过程的观测值，不宣称跨文件原子。
- 单节点总量上限 16,384 行；达到上限后停止解析并持久化 `truncated=true`，已计数部分保留，
  此时 `total` 是下界而非全量。
- raw 记录 `samples_expected=1` 与 `samples_actual`；聚合点的 `samples_expected` 为名义桶分钟数
  （5m=5、daily=1440），仅窗口首尾的不完整桶按实际覆盖分钟缩减；桶内缺口不降低 `expected`。
  `samples_actual` 为实际观测分钟数，缺口只体现在它和 gap 上。
- 缺失分钟是 gap，不是 0。
- 非 Linux 节点全部返回 `unsupported`，不产生零值记录。
- 单个 `/proc` 文件或部分行解析失败时标记 `partial`，可读部分照常写入；两个文件都不可读时本分钟为 gap。

#### 数据模型与 Rollup

- 每个节点每分钟至少一条稀疏记录，元素为 `(class, subject, count)`；按 `shard_index` 可分多条，
  本地使用二进制编码。
- 写入时同时计算四类小计与 `total`。
- 5m 与 daily Rollup 由 raw 生成，禁止跨源累加或改写已确认记录。
- 每个桶保存逐 subject 的 `min/avg/max/samples`；5m 桶另存逐分钟 subject 值与四类小计，
  daily 桶另存逐分钟 `total`，保证 `total` 的极值由「同一采样过程先求和、再统计」得出。
- 禁止用 `sum(per-subject max)` 代替 `total.max`。
- daily 层不保存逐分钟类别小计，因此 daily 类别 series 的 `min/max` 由逐 subject 极值求和
  近似，必须带 `approximate=true`；端点筛选不产生新的 total series。
- daily 类别 series 是近似拆分，不得用其求和校验 total；total 以逐分钟 total 为准。
- 节点级 total 在四类完整样本上保持精确；导入分钟是下界。1m / 5m 类别 series 与四类完整
  样本的节点级 total 为 `approximate=false`。

#### 保留与投递

- 保留：节点本地 1m raw 与 5m 各 7 天；History Repository 只保留 daily 90 天。1m 不得延长到
  7 天以外，Repository 不得被要求保存 1m / 5m。
- 新 History Stream 为 `node_tcp_connections.v1`，必须使用专用 reducer；通用 hash reducer 视为回归。
- `connections.v1` 继续投递 latest 摘要，仅作旧读者兼容，不承担历史。
- `node_tcp_connections.v1` 的未确认 journal 是既有全局 128 MiB / 20,000 段硬上限之下的流级
  预算：16 MiB 或 10,000 段，先到者为准。80% 持久化 `capacity_suspended` 并报告
  `journal_capacity_guard`，停止新采集，低于 60% 恢复。
- History Repository 配额为 `min(2 GiB, repository quota 的 25%)`；新增节点、启用仓库或恢复历史前
  按 20 MiB × 可采集节点数预检，失败返回容量拒绝，不缩短保留或丢弃旧数据。
- 单节点 20 MiB 预留覆盖 daily 90 天与 SQLite / WAL / 索引余量；daily 每节点每天最多 4 个
  6 KiB 分片（total 与四类小计在 shard 0，subject 明细分片），daily payload 上界约 2.2 MiB。
  2 GiB 配额上限最多覆盖 102 个节点；超过时预检必须拒绝，不得自动降低单节点预留。
- 本地 1m / 5m 存储预算为 96 MiB / 节点；达到 80% 停止新采集并形成 gap，不裁剪已确认的
  7 天数据。
- 升级时必须把现有 `tcp_connection_usage.json` 的 7 天记录按 `(minute, endpoint_id)` 幂等导入为
  `inbound`，不重采样、不丢失已观测分钟。
- 导入分钟只观测了 `inbound`，必须标记 `classes_observed=["inbound"]`；这些分钟不得被当作
  完整四类样本，查询按 `quality=partial`、`reasons=["legacy_inbound_only"]` 降级。
- 继续写 7 天 `tcp_connection_usage.json` 兼容镜像；旧 XP 仍可读取，降级只损失超过 7 天的数据。
- 单条 payload 上限：1m 4 KiB、5m 6 KiB、daily 6 KiB；超出按 subject 排序分片，payload 带
  `shard_index` / `shard_count`。`record_key` 必须为
  `node-history:node:{node_id}:{resolution}:{bucket_start}:{shard_index}`，不得只写序号。
- total 与四类小计只出现在 shard 0；subject 明细可分布在其余分片。缺任一 shard 时该桶必须
  标 `partial`，不得用已有分片求和伪装完整值。
- daily subject 明细超过 4 个分片预算时按 subject 排序截断；total / 四类小计仍完整，响应带
  `subjects_truncated=true` 与 `subjects_omitted=true`，`subjects_next_cursor=null`。
- 端点删除不改写已确认的 total / classes / subjects；写入 endpoint tombstone
  `node-history:endpoint:{endpoint_id}:`，查询时把被覆盖的 subject 移出 `subjects`，并在
  `deleted_subjects` 返回。total / classes 仍是节点历史事实，允许大于返回的 subject 之和。
- 节点删除使用 `node-history:node:{node_id}:` 前缀 tombstone，清理全部本地与仓库历史；
  snapshot install 按同一 tombstone 集重建可见视图。

#### API

- `GET /api/admin/nodes/{node_id}/tcp-connections` 保留：24h/7d 由目标节点本地或 internal local
  返回；30d/90d 由接收请求的节点直接查询 ready Repository，不依赖目标节点在线。
- `GET /api/admin/_internal/nodes/tcp-connections/local` 只接受 24h/7d；长期窗口返回 `400`。
- 长期仓库查询使用仓库历史预算，不复用 3 秒 peer 读超时。
- 查询参数：`window=24h|7d|30d|90d`、`resolution=auto|1m|5m|daily`、可重复 `class`、
  可重复 `endpoint_id`。
- `auto` 映射：24h → 1m、7d → 5m、30d/90d → daily；显式分辨率不得细于保留层。
- 查询参数另含 `subjects_limit`（默认 20，最大 100）；`subjects` 是分页视图，超出时返回
  `subjects_truncated=true` 与 `subjects_next_cursor`，total / classes 始终完整。
  `subjects_omitted=true` 表示入库时已丢弃明细，不能通过 cursor 取回。
- 响应恒含 `total`、按类别聚合的 `classes` 与按主体展开的 `subjects`；每个 series 独立带
  `approximate`。`endpoint_id` 过滤只收窄 `inbound` 的 class/subject，total 与其他类别不变。
- 响应必须包含 `quality=complete|partial|unsupported`、`reasons[]`、`watermark`、`gaps[]`、
  `truncated`、`truncated_buckets[]`、`deleted_subjects[]`、`subjects_truncated`、
  `subjects_omitted`、`subjects_next_cursor`、`resolution`。
- `reasons` 至少包含 `repository_unavailable`、`runtime_identity_unresolved`、
  `legacy_inbound_only`、`capture_suspended`；`gaps[].reason` 使用同名集合。
- 类别或主体非法、分辨率越界、窗口非法返回 `400`；24h/7d 的目标节点不可达沿用现有 fan-out
  错误语义；30d/90d 在 Repository 不可用时返回 `partial` + `repository_unavailable`，
  而非节点不可达。

#### UI

- `TCP connections` tab 保留；默认画 `total`，类别拆分可切换并可堆叠。
- `outbound_other` 默认聚合展示，tooltip 给 UID；不展开任意进程列表。
- gap 不连线，`partial`、`truncated`、`unsupported`、`capture_suspended` 显示明确状态。
- 24h/7d 的端点多选只影响 `inbound`；total 与其他类别不受影响。
- 沿用现有 ECharts 阶梯线、主题与 tooltip 视觉合同；新增类别使用固定中性色序列。

### SHOULD

- 采样器复用 resource monitoring 已解析的固定 runtime 身份，不新增提权路径。
- 采样节拍必须有独立的每分钟机会，不因 quota 轮询间隔被放宽到 60 秒以上而丢分钟。
- 分类与编码保持在单次顺序扫描内，单分钟采样耗时有固定上限。
- 本地读取路径与当前值查询不经过仓库；30d/90d 由接收请求的节点查询最完整健康的 ready Repository。

### COULD

- 后续独立规格增加按远端目标或端口的出站聚合，需要 access log 或 eBPF。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

1. 固定分钟节拍（独立于 quota 轮询）读取两个 `/proc` 文件，逐行分类，得到四类 subject 计数与 total。
2. 记录先写本地 SQLite raw，再由 Source Delivery Journal 投递。
3. 节点本地生成 1m / 5m；Repository 幂等接收 1m，生成 daily，只保留 daily。
4. Web 查询 total 与拆分；本地窗口读本地，长期窗口读 Repository。
5. 端点删除写 endpoint tombstone 并在查询期过滤 subject；节点删除清理全部历史；
   snapshot install 按 tombstone 集重建视图。

### Edge cases / errors

- 无连接是成功样本，值为 0；无样本分钟才是 gap。
- 只有地址与端口同时匹配配置的控制面监听项才归 `control_plane`，双侧命中不重复计数。
- runtime 未运行：`outbound_runtime` 自然为 0，不返回 unsupported。
- UID 不可解析或容器身份文件缺失：保持已解析行的计数，runtime 部分降级为 `outbound_other`，
  返回 `partial` 与 `runtime_identity_unresolved`。
- 超过 16,384 行：`truncated=true`，`quality=partial`，`total` 明确为下界。
- 目标节点离线但 Repository ready：30d/90d 仍返回仓库历史；24h/7d 仍返回节点不可达。
- SQLite 不可写、journal 满或仓库拒绝容量：进入 suspension，raw 缺失区间成为 gap。
- 24h 内节点重启：重启后首分钟重新采样，不补跑，不伪造旧窗口。
- 旧节点不认识新 schema 时继续读 `connections.v1`；新 Web 按 capability 降级窗口与类别。

## 接口契约（Interfaces & Contracts）

- node TCP connection admin/local API：HTTP JSON，external/internal，Modify；使用方 Web/backend。
- `node_tcp_connections.v1`：signed source history，internal，New；使用方 Repository。必须注册到
  `KNOWN_SCHEMAS`、source/receiver stream 映射、查询投影与删除 marker。
- `connections.v1`：signed source history，internal，Keep；兼容旧 Repository 读者。
- category / resolution 参数：HTTP query，external/internal，New；使用方 Web。

响应结构（示意）：

```text
{
  "node": { "node_id": "01J..." },
  "window": "30d",
  "resolution": "daily",
  "window_start": "2026-10-26T00:00:00Z",
  "window_end": "2026-11-25T00:00:00Z",
  "quality": "complete",
  "reasons": [],
  "truncated": false,
  "truncated_buckets": [],
  "deleted_subjects": [],
  "subjects_truncated": false,
  "subjects_omitted": false,
  "subjects_next_cursor": null,
  "watermark": "2026-11-24T23:59:00Z",
  "gaps": [{ "start": "...", "end": "...", "reason": "capture_suspended" }],
  "total": { "approximate": false, "points": [Point] },
  "classes": [
    { "class": "inbound", "approximate": true, "points": [Point] }
  ],
  "subjects": [
    { "class": "inbound", "subject": "01J...", "approximate": false, "points": [Point] }
  ]
}
```

`Point`：

```json
{
  "bucket_start": "2026-11-25T10:05:00Z",
  "min": 3,
  "avg": 7.2,
  "max": 14,
  "samples_expected": 5,
  "samples_actual": 5,
  "truncated": false
}
```

`truncated_buckets` 按 `bucket_start` 列出至少一分钟截断的桶；对应 `Point.truncated=true`，
该点的 total / 类别值是该桶下界。`approximate=true` 只表示聚合形状由逐 subject 极值重建，
截断下界由 `truncated` 单独表达。

`deleted_subjects` 为 `[{class, subject}]`；`inbound` 的 subject 即 `endpoint_id`。
`subjects_next_cursor` 是不透明值，只在本响应窗口与过滤条件下有效。
`subjects_truncated` 只表示明细分页，不降低 total / classes 的 quality；`deleted_subjects`
也不改写 total / classes。
`subjects_omitted=true` 表示入库时已按分片预算丢弃明细；此时 cursor 为 null。

写路径容量拒绝返回 `409 node_tcp_connections_capacity_rejected` 与
`required_bytes`/`allowed_bytes`；读路径返回 `400 invalid_request`、`404` 与既有 peer failure 合同。
capability `admin.node-tcp-connections-history` 为 additive。

## 验收标准（Acceptance Criteria）

- Given 一入一出两个 socket，When 采样，Then `total=2`、`inbound=1`、`outbound_runtime=1`。
- Given XP 绑定 `0.0.0.0:62416`，When 浏览器连接到 XP API，Then 计入 `control_plane` 与 total。
- Given 无关系统进程的连接，When 采样，Then 计入 `outbound_other`，UID 可查。
- Given A、B 峰值出现在不同分钟，When 查询 7d，Then `total.max < max(A) + max(B)`。
- Given 5m 桶内只有 3 个观测分钟，When 聚合，Then `samples_expected=5`、`samples_actual=3`。
- Given 超过 16,384 行，Then `truncated=true`、`quality=partial`，分类计数仍在。
- Given 非 Linux，Then `unsupported`，无零值曲线。
- Given journal 达 80%，Then `capacity_suspended` / `journal_capacity_guard` 且停止新写入；
  降至 60% 以下恢复。
- Given 24h 窗口选择 3 个 endpoint，Then 只有 `inbound` 拆分被过滤，total 不变。
- Given 30d 查询并过滤 endpoint，Then daily 类别 series `approximate=true`，total 不变。
- Given 目标节点离线且 Repository ready，When 查询 30d，Then 返回仓库历史而不是节点不可达。
- Given 升级前存在 7 天 JSON 历史，When 启动完成，Then 导入为 `inbound`，查询标
  `partial` + `legacy_inbound_only`。
- Given endpoint 删除，Then `subjects` 不再返回该 endpoint，`deleted_subjects` 列出它，
  total / classes 不变。
- Given 90d 窗口内一分钟截断，Then 仅对应 `Point.truncated=true`，顶层 `truncated=true`。
- Given `subjects` 超过 `subjects_limit`，Then 返回 `subjects_truncated=true` 与 cursor，
  total / classes 完整。
- Given daily 明细超过 4 个分片预算，Then `subjects_omitted=true` 且 cursor 为 null，
  total / classes 完整。
- Given 本地 1m / 5m 存储达 80%，Then 停止新采集并形成 gap，已确认的 7 天数据不裁剪。

## 质量门槛（Quality Gates）

- Backend：`cargo fmt` / `cargo clippy -- -D warnings` / `cargo test`
- Web：`cd web && bun run lint && bun run typecheck && bun run test`
- Storybook：`cd web && bun run test-storybook`
- 视觉证据：total 默认态、类别堆叠态、gap/partial/truncated/unsupported 态。

## 文档更新（Docs to Update）

- `docs/desgin/api.md`：窗口、分辨率、类别、质量、分页与 endpoint 删除后可见语义。
- `docs/ops/README.md`：全站 socket 采样范围、容器网络命名空间边界、容量预检。
- `docs/specs/node-tcp-connection-count/SPEC.md`：扩展并取代其端点求和展示语义。

## 计划资产（Plan assets）

- Directory: `docs/specs/tcp-connection-history/assets/`
- In-plan references: None.

## Visual Evidence

待实现；证据要求见「质量门槛」。
