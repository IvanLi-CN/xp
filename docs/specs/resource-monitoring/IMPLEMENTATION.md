# Resource Monitoring implementation

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout
> 相关事实。

## 当前状态

首版节点级资源监控已接入 XP runtime、admin/internal API 和 Web。采样器以 15 秒 Skip
节拍运行，保留 240 个内存样本，并在 `${XP_DATA_DIR}/resource_metrics.sqlite3` 中以 WAL
保存分钟 Rollup、policy、告警状态和 capture gap。
host-managed 节点读取宿主机 `/proc`/statvfs；容器节点读取 cgroup v2 的 CPU 与 memory
limit/current，不能把宿主机内存总量伪装成容器值。旧节点通过 additive
`admin.resource-monitoring` capability 继续工作并显示 `unsupported`。

## 已冻结的合同

- Resource Domain、指标集合、采样频率、容量护栏、专用 history reducer、policy、API 和 UI
  均由 `./SPEC.md` 与 `./contracts/api.md` 定义。

## 已覆盖的实现面

- 固定 Resource Domain、四个运行角色、字段级 capability/reason code、Linux
  `/proc`/cgroup/statvfs reader。XP 读取自身；systemd 读取固定 unit cgroup，OpenRC 读取固定
  supervisor PID 文件的直接子进程，官方单镜像容器读取入口写入并以启动时间校验的私有身份文件。canary 是 XP
  进程内任务，启用时显式返回 `runtime_not_separable`，不复制 XP 指标。
- 15 秒单调 Skip 采样、240 点内存环、分钟 Rollup（min/mean/max/last、expected/captured）
  和 counter reset 饱和处理。
- 独立 WAL Resource Store，分钟留存上限、policy revision、资源告警状态和 bounded capture
  gap。
- policy 写入通过 `SetResourcePolicy` Raft state-machine command 完成 revision CAS；节点和固定
  role override 先做字段级校验，再由本地 evaluator 合并，SQLite 只保留可用缓存。
- Resource history payload 使用固定 metric code 的紧凑 wire 格式，按 resolution 执行
  2 KiB/1 KiB/768 B 预算；source delivery journal 仅在实际投递确认后清理资源 pending
  行，并保留 32 MiB/10000 项上限。
- admin/internal current、recent、history、policy 路由；签名 Mesh fan-out 的
  partial/unreachable 返回；history 查询按 `resource_metrics.v1` schema 过滤并消费有界
  分页；admin.alerts 资源告警扩展。
- 集群总览和 Node Details Resources Tab，current 15 秒轮询、history 30 秒轮询，以及
  unsupported/partial/suspended 状态。Resources Tab 始终显示四个固定 Domain 图；四个角色卡可展开其固定
  五图详情（CPU、RSS/PSS、读/写、FD、线程），一次只查询一个角色；底层仍固定查询七项单指标历史。
- systemd/OpenRC/Docker 共用完整 `XP_DATA_DIR`，升级路径必须保留
  `resource_metrics.sqlite3`。
- Repository membership 变更和新节点加入在提交前按 82 MiB/可采集节点和 40%/4 GiB quota
  上限执行资源历史 capacity preflight，并返回稳定 `resource_history_capacity_rejected` 详情。

## 后续收敛面

1. 将资源历史的 82 MiB/节点容量预检继续接入历史恢复专用控制流程。
2. 增加三种升级 fixture、Playwright 和 shared-testbox 性能基线，并把 Storybook 截图证据
   持久化到本主题的 assets 目录。

## Remaining gaps

- 当固定 systemd/OpenRC/container 身份不可读取、PID 已替换或权限不足时，Xray/cloudflared 的字段保持
  `unsupported`；没有新增 PID 枚举或提权路径。canary 始终不伪装为独立进程，启用时以
  `runtime_not_separable` 降级。
- 本地 history 已实现 minute/15-minute/hour 查询和 gap 质量；ready History Repository 已通过
  既有 signed query selector 提供资源流查询。新节点自动加入路径和恢复专用路径的 quota
  preflight 仍需后续收敛。
- 首版不拥有读取受限 PSS/I/O 的 root 权限；这些字段必须通过 capability 降级。
