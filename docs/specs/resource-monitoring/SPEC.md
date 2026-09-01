# 资源监控（Resource Monitoring）

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，主题局部演进见 `./HISTORY.md`。持久决策的完整取舍见关联 ADR。

## 背景 / 问题陈述

XP 当前可观测节点运行状态、服务可用性、流量与 TCP 连接数，但不能回答托管运行栈或其所在执行边界是否受到 CPU、内存、磁盘容量或 I/O 压力。已有 History
Repository 能安全保存有界、签名的长期历史，但其通用聚合器不会保留资源数值的语义。

资源监控必须同时适用于 host-managed systemd、OpenRC 与官方单镜像 Docker/Compose 节点，且不能用高频写入、无限标签、无限 backlog 或新增
root 常驻服务来解决可观测性问题。

## 目标 / 非目标

### Goals

- 持续观测每个节点的 Resource Domain 与 Managed Runtime Stack 的固定资源指标。
- 提供当前快照、短期高分辨率趋势、长期语义 Rollup、质量信息与站内 Resource Alert。
- 通过受限的 Source Delivery Journal 和 History Repository 保存长期历史，不写入 Raft。
- 对权限、内核、cgroup 或文件系统限制明确返回 `supported`、`partial` 或 `unsupported`。
- 在资源紧张的节点上保持采样、内存、磁盘和网络成本有界。

### Non-goals

- 不提供任意进程选择、任意 label、任意查询、Prometheus `/metrics`、外部 TSDB 或脚本执行。
- 不收集任意第三方进程、完整命令行、环境变量、网络五元组或重复的业务流量统计。
- 不新增 XP 控制的 root daemon、sudo、doas、polkit 或自动提权路径。
- 不把 15 秒原始样本长期持久化、跨升级回填，或在容量不足时静默缩短留存。
- 不发送邮件、Webhook、IM 通知，不因 Resource Alert 自动重启服务或改变节点配置。

## 范围（Scope）

### In scope

- Linux Resource Domain 的 CPU、内存、Swap、文件系统容量/inode、可归属磁盘 I/O 与 I/O wait。
- `xp`、Xray、cloudflared、XP-owned canary 四个固定运行角色的 CPU、RSS、PSS、I/O、FD、线程指标及字段级 Measurement
  Capability。
- 每 15 秒采样、内存环形窗口、分钟/15 分钟/小时语义 Rollup、SQLite 持久化、History Repository 投递与容量护栏。
- 集群资源总览、节点 Resources Tab、专用 API、历史质量和站内 Resource Alert。
- 集群默认阈值及显式 node/role override、滚动升级兼容和三种一等部署形态。

### Out of scope

- Windows、macOS 或任意 Docker host 全局资源的主动采集。
- 任意挂载点、块设备、PID 或容器的枚举；只采集 root 与 `XP_DATA_DIR`，并按文件系统 identity 去重。
- 未经显式 role override 的单进程资源阈值；不同规格节点不能由 XP 自动调参。
- 外部告警投递、自动扩缩容、自动故障恢复或通用仪表盘构建器。

## Related ADRs

- [ADR 0009](../../adr/0009-bounded-resource-monitoring-history.md)

## 需求（Requirements）

### MUST

#### Measurement model

- Host-managed 节点的 Resource Domain 是宿主机；官方单镜像容器节点的 Resource Domain 是该工作负载 cgroup。
  容器内不可把宿主机总量伪装为容器总量。
- 每个样本只包含固定的 Domain 指标和最多四个固定运行角色。角色未托管、已禁用、权限不足与平台不支持必须相互区分。
- Domain 指标包含 CPU busy 与 I/O wait、load average、内存总量/可用量、Swap、root 与 `XP_DATA_DIR`
  文件系统容量/可用量/inode，以及在该 Domain 可归属时的读写速率。
- 每个运行角色包含 CPU 利用率、RSS、PSS、读写速率、FD 数和线程数。单项无法读取时仅该字段降级，不得让其他可读字段丢失。
- 值为零只表示一次成功测量得到的零。不可测量的字段必须省略 `value`，并返回 capability 与稳定 reason code。

#### Sampling and local state

- 采样器以 15 秒单调节拍运行；错过节拍时跳过而非补跑或并发堆积。一次采样不得启动 shell、外部命令、递归扫描 `/proc` 或枚举任意 PID。
- 每个 tick 最多解析四个运行角色、两个去重后的文件系统和为 Domain 定义的固定 Linux 数据源。采样器必须有固定时限，并在超时后记录缺失样本，而非阻塞控制面任务。
- 最近 240 个 15 秒样本仅保留在内存中，提供约一小时的短期趋势；进程重启后不伪造或回填该窗口。
- 每个 UTC 分钟生成一个 Resource Rollup，记录该分钟预期与实际样本数。gauge 保留 `min`、`mean`、`max`、`last`，counter 保留安全
  delta，capability 保留最差状态。
- `${XP_DATA_DIR}/resource_metrics.sqlite3` 使用 WAL，只保存分钟 Rollup、持久 capture gap 与 Resource Alert
  状态；不得保存 15 秒原始样本。
- 已确认 Rollup 仅在本地保留 24 小时，未确认 Rollup 仍受 source journal 上限约束。每节点 Resource Alert transition 最多保留
  30 天或 1,000 条，以先到者为准。
- Resource Store 的失败不得影响 XP、Xray、cloudflared、Raft、join 或升级。

#### History, retention, and pressure

- 每个节点每分钟最多向 `resource_metrics.v1` Resource History Stream 投递一个 Rollup。它复用现有 Source Delivery
  Journal 的签名、cursor、ACK、固定页与 oldest-first drain 行为。
- Resource History Stream 必须有专用 reducer；它不能使用会把资源 payload 简化为 hash 和 record count 的通用 reducer。
  15 分钟与小时 Rollup 必须保留 min/mean/max/last、counter delta、capability 和预期/实际采样数。
- minute payload 最大为 2 KiB canonical，15-minute payload 最大为 1 KiB，hour payload 最大为 768 B。固定角色、
  固定文件系统、无动态 label 是这些上限的前提。
- 保留窗口依次为 14 天 minute、随后 90 天 15-minute、随后 365 天 hour。每节点最大有效载荷约 54 MiB；为 SQLite 索引、WAL 和元数据预留
  50% 后，容量计划按 82 MiB/节点计算。
- Resource History Stream 的配额为 `min(4 GiB, History Repository quota 的 40%)`。在新增节点、启用仓库或恢复历史写入前，
  系统必须以 82 MiB 乘以可采集节点数进行预检。预检失败必须返回容量拒绝，而不是缩短 retention 或丢弃旧数据。
- 本地未确认资源 history journal 的上限是 32 MiB 或 10,000 条，以先到者为准。达到上限、Resource Store 不可写或仓库拒绝容量时进入
  Resource Capture Suspension。
- Capture Suspension 期间当前内存样本继续更新，长期历史明确为 `partial`。恢复后以最后成功持久化的 minute 与首个恢复 minute 推导并写入一个有界
  gap，不能把缺失区间表示为零。
- 历史查询必须返回 `complete`、`partial` 或 `local_only`，并包含 coverage、watermark、gap、freshness、
  source/Repository capability 和响应是否截断。

#### Alerts and policy

- Resource Alert Policy 是 Raft 中的 revisioned cluster default，加显式 node/role override。它只保存阈值和启用状态，
  不保存样本、告警事件或实时值。
- 默认 Domain CPU busy：`>=85%` 持续 10 分钟为 warning，`>=95%` 持续 5 分钟为 critical。
- 默认 Domain 可用内存：`<=10%` 持续 10 分钟为 warning，`<=5%` 持续 5 分钟为 critical。
- root/`XP_DATA_DIR` 空间或 inode 使用率 `>=85%` 为 warning，`>=95%` 为 critical。
- 可读的 I/O wait `>=20%` 持续 10 分钟为 warning。Resource Capture Suspension 是 warning。
- 每角色资源阈值默认关闭，只有存在 cgroup limit 或管理员显式 role override 时才可启用。
- 告警仅在进入、升级、恢复时发生状态转换，以避免每个样本重复创建。活动告警通过扩展后的 `admin.alerts` 与现有状态 SSE 可见；恢复事件保留在 Resource Store
  的有界本地状态中。
- Resource Alert 不得触发自动重启、配置修改或外部通知。

#### API, UI, and compatibility

- 资源 current、history 和 policy API 的字段、错误与认证以 `./contracts/api.md` 为准。
- 集群 current 聚合遵循现有 internal signature fan-out：不可达节点返回 `partial` 与 `unreachable_nodes`，
  可达节点结果继续返回。
- 历史由专用 Resource History API 从最完整健康的 ready History Repository 查询；不得让 Web 解释 generic `unknown[]`
  Repository records 或扫描所有 schema。
- Web 提供集群资源总览与 Node Details 的 Resources Tab。当前值每 15 秒轮询，历史每 30 秒轮询；不通过 SSE 传输 15 秒原始样本。
- capability `admin.resource-monitoring` 是 additive。未升级或不支持的节点显示 `unsupported`，不把整个集群标记为
  down/degraded。没有历史回填，采样从新二进制首次成功 tick 后开始。
- host-managed systemd、OpenRC 与官方单镜像 Docker/Compose 的升级都必须保留
  `${XP_DATA_DIR}/resource_metrics.sqlite3`。数据库缺失、迁移失败或字段不可读必须可见，且只能降级资源监控本身。

### SHOULD

- 采样器优先使用不需要额外权限的 cgroup 或 `/proc` 数据源。PSS/I/O 不可读时保留 RSS/CPU 等可读指标并给出原因。
- 历史查询自动选择所需的最粗 resolution，最多返回 1,500 点；短期当前查询只从节点的内存窗口读取。
- Node Details 在图表、表格和 alert 上同时展示 Resource Domain、capture state 与字段能力，避免把 container cgroup 与
  host 混淆。

### COULD

- 在后续独立规格中，以 owner 明确授权为前提设计可审计的最小 root snapshot helper，用于当前无法读取的 role PSS/I/O；该 helper
  不属于本主题的首版交付。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

1. 每个节点的采样器读取固定的 Domain 与运行角色数据，更新内存环形窗口，并在 UTC 分钟边界写入一个语义 Rollup。
2. Resource Store 将可投递 Rollup 交给 Source Delivery Journal；Repository ACK 后按现有有界投递路径释放 pending 数据。
   Repository 对资源流执行专用分层聚合和 retention。
3. 集群总览向每个节点读取 local current snapshot；Node Details 的当前窗口读取同一节点，长期趋势读取最完整的 ready Repository。
4. Policy evaluator 在本地从完整分钟 Rollup 判断持续阈值，写入 Resource Alert 状态转换；管理页面从现有 alert 聚合与状态 SSE
   获得活动告警和恢复事件。

### Edge cases / errors

- 运行角色刚重启、PID/cgroup 更换或 counter reset 时，rate 使用新基线并记录 partial 样本；不产生负 delta 或虚假的峰值。
- `/proc`、cgroup、diskstats 或 statvfs 不可读时，受影响 measurement 返回 `unsupported`；只读到部分字段时返回 `partial`。
  不因该字段失败停止整个采样器。
- cloudflared、canary 或相应 role 未托管时，role 状态为 `not_managed`，不同于 `unsupported`。
- 采样器超时、时钟跳变、数据库不可写、journal 满、Repository 低空间或 quota 拒绝时，历史质量必须明确降级；当前窗口继续可用的前提是不阻塞该 tick。
- 远端节点不可达时，current 总览保留最后可用节点数据并报告 partial；历史 Repository 不可达时返回 local_only 或 partial，绝不假定
  complete。
- 旧节点、旧仓库或不认识资源 schema 的节点保持原有行为；新 Web 根据 capability 隐藏交互，而不是将其视为零或错误。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

- Resource current APIs
  - 类型：HTTP JSON；范围：external/internal；变更：New。
  - 使用方：Web/backend；提供 fan-out 和 local snapshot。
- Resource history API
  - 类型：HTTP JSON；范围：external；变更：New。
  - 使用方：Web；从 ready Repository 执行专用查询。
- Resource policy API
  - 类型：HTTP JSON + Raft；范围：external；变更：New。
  - 使用方：Web；管理 revisioned default 与 override。
- Resource History Stream
  - 类型：signed source history；范围：internal；变更：New。
  - 使用方：Repository；采用 `resource_metrics.v1` 专用 reducer。
- Resource alerts
  - 类型：HTTP JSON + SSE；范围：external；变更：Modify。
  - 使用方：Web；向 `admin.alerts` 增加 additive variants。

以上接口的详细契约见 [Resource Monitoring API and stream contract](./contracts/api.md)。

## 验收标准（Acceptance Criteria）

- Given 一个 host-managed 节点拥有可读的 `/proc` 数据，When 连续采样一分钟，Then current API 返回四个计划样本形成的 Rollup，
  且无动态 PID/mount label。
- Given 一个 Docker/Compose 节点，When 查询其资源，Then Resource Domain 明示为 cgroup，且不会把宿主机容量表示为容器资源。
- Given Xray PSS 不可读而 RSS/CPU 可读，When 读取 current 或 history，Then PSS 为 `unsupported`、节点为
  `partial`，RSS/CPU 仍保留，且没有零值伪装。
- Given Repository 不可达直到本地 journal 达到任一上限，When 继续采样，Then 当前窗口仍更新、历史进入 Resource Capture
  Suspension 并在恢复后返回明确 gap。
- Given 50 个可采集节点，When 预估资源历史超过分配的 Resource History Stream 配额，Then 系统拒绝新的资源历史容量，而不缩短 retention
  或影响集群控制面。
- Given 一个分钟 Rollup 年龄跨越 retention 边界，When Repository 压缩它，Then 15-minute/hour 结果仍保留数值聚合、counter
  delta、capability 和 capture completeness。
- Given CPU 连续十分钟高于 85%，When policy evaluator 完成该分钟，Then `admin.alerts` 出现一个 resource warning；
  条件恢复后只产生一个恢复转换。
- Given 旧节点与新节点混合滚动升级，When 打开集群资源总览，Then 旧节点显示 `unsupported`，新节点可显示 current/partial，集群不因此被标为
  down。

## 验收清单（Acceptance checklist）

- [ ] 固定指标集合、Resource Domain 与字段能力已被明确描述。
- [ ] 采样、缓存、Rollup、容量、gap 与 retention 的长期行为已被明确描述。
- [ ] API、告警与 UI 契约已写清楚。
- [ ] 三种部署形态、权限降级、滚动升级和存储失败已覆盖。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests：Linux 数据源解析、counter reset、capability 分类、minute reducer、alert hysteresis、SQLite
  migration/cap、payload size 和 retention reducer。
- Integration tests：journal 背压与 gap、Repository quota/recovery、schema-aware history query、
  fan-out partial、Raft policy revision 与旧节点 capability。
- Shared testbox：在 256 MiB/no-swap Linux 节点上执行 systemd、OpenRC、单镜像容器路径；连续 15 分钟验证：采样器 p95 wall
  time 不超过 250 ms、稳态 RSS 增量不超过 1 MiB、CPU 不超过一个 CPU 核的 0.5%、每节点每分钟最多一次 Resource Store commit。
- 长时间测试：至少一个 Resource History Stream 到达 32 MiB journal、Repository quota 边界和恢复 drain，
  验证控制面与现有采集不受影响。

### UI / Storybook

- Stories：cluster resource overview、host/cgroup domain、partial/unsupported、capture suspended、
  active/recovered alert 和历史质量状态。
- Playwright：总览筛选、节点 Resources Tab、resolution 切换、旧节点 capability 与 alert recovery。

### Quality checks

- `cargo fmt --check`、`cargo clippy -- -D warnings`、相关 Rust 单测与集成测。
- `cd web && bun run lint && bun run typecheck && bun run test`，以及 Storybook、Playwright、style
  budget。

## 实现前置条件（Definition of Ready / Preconditions）

- `resource_metrics.v1` payload、大小上限、专用 reducer 与历史查询 fixture 已冻结。
- Resource Alert Policy 的 revision、默认阈值、override precedence 与稳定错误码已冻结。
- Linux host/cgroup capability matrix、三种部署形态的测试夹具和 256 MiB 资源基线可用。
- History Repository 容量预检和 Resource History Stream quota 已纳入其控制与状态模型。

## 文档更新（Docs to Update）

- `docs/ops/README.md` 与 Docker 部署文档：Resource Domain、持久卷、能力降级与容量告警。
- `docs/desgin/api.md` 与工作流文档：资源 API、历史质量和 alert semantics。
- `docs/specs/nmgq8-managed-stack-64m-memory/SPEC.md`：明确固定 Resource Monitoring 不等同于不受约束的通用
  metrics 平台。
- `AGENTS.md`：实现时同步三种部署形态的 Resource Store 保留与 capability contract。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：部分 hardened host 无法提供 role PSS/I/O；首版以字段能力降级保持正确性，而非扩大权限。
- 风险：资源历史会与现有历史流争用默认 10 GiB Repository 配额；专用 quota 和预检是硬门禁。
- 风险：采样器本身可能在小内存节点上成为噪声；固定读取范围、内存环和性能质量门禁限制其成本。
- 需要决策的问题：None。
- 假设：一等部署目标均为 Linux，且至少能提供 Resource Domain 的基础 CPU/内存/文件系统读数；不满足时以 `unsupported` 呈现。

## 参考（References）

- `../cluster-history-repositories/SPEC.md`
- `../uptime-monitoring/SPEC.md`
- `../9vmap-node-service-observability/SPEC.md`
- `../m4n7c-node-tcp-connection-count/SPEC.md`
- `../nmgq8-managed-stack-64m-memory/SPEC.md`
