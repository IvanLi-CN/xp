# 服务监控（Service Monitoring）实现合同

> 规范正文见 `./SPEC.md`；HTTP wire shape 见 `./contracts/api.md`。

## Control plane

- `src/uptime_monitor.rs` 定义 Monitor、不可变 revision、状态、Observation、
  固定 32 桶延迟直方图和质量计算。
- `src/state.rs` 把 Monitor 写入 DesiredState/Raft；schema migration 保持旧
  state 可启动。创建和编辑在下一个 UTC Slot 生效，pause/resume/delete 都通过
  revision-safe lifecycle command 收敛。
- `src/http/service_monitors.rs` 在 Leader 写路径执行 300
  `monitor x observer` slots/min admission；超额返回
  `observation_budget_exceeded`，不减少 Observer Set。

## Execution and safety

- scheduler 使用 UTC 对齐 Slot、每节点 32 个计划并发和 skip missed tick；不补跑
  重启前的 Slot，也不自动重试。
- HTTP/HTTPS 使用无代理 client，最多 3 跳 redirect；每一跳都重新解析并拒绝
  非公网地址。HTTPS 不允许协议降级。
- HTTP 连接超时为 5 秒，总超时为 10 秒；literal body matcher 最多读取 64 KiB，
  配置值最多 256 bytes。
- TCPING 只运行一次 Tokio TCP connect 并立即关闭。
- PING 在 Linux 先使用 ICMP datagram socket，必要时只退回同语义 raw socket；
  不调用外部 `ping` binary，也不以 TCPING 代替 ICMP。无能力时 result 为
  `unsupported`。
- ad-hoc run 受每 token 每分钟 10 次、每 Observer 4 个并发限制；它独立于
  scheduled idempotency，且不进入 availability/coverage 分母。

## Persistence and query

- 每个 node 使用 `${XP_DATA_DIR}/uptime.sqlite3` 的 WAL-backed pending store。
  计划检查只在 Repository 已 ready 且 capture store 可接受记录时开始。
- pending backlog 在 80%（64 MiB 或 100,000 条的任一限制）进入
  `capture_suspended`，低于 60% 才恢复。该状态会停止新检查，不会丢弃已执行结果。
- worker 将 `service_monitor_observation.v1` 作为
  `service_monitor_observation-v1` signed Source Delivery stream 投递；ack 后才标记
  本地 observation 已入队。
- History Repository 沿用现有 SQLite row store、签名 cursor/ack 和 fork isolation。
  retention 将 uptime payload 合并为 minute、five-minute、hour bucket，并保留
  expected/executed/outcome、错误统计和可合并 latency histogram。
- history API 根据查询窗口在 `1m`、`5m`、`1h` 之间选择粒度，最多返回 1,500
  点，且明确返回 `complete`、`partial` 或 `local_only`、coverage、watermark、gap、
  skew 和 freshness。

## API and Web

- 后端提供 monitor list/create/get/patch/delete、status、history、run 和 run status
  routes；定义、fixture 和 Web Zod schema 共同使用 internally-tagged `target.kind`
  wire shape。
- Web 注册一级“Service monitoring”导航以及 overview、new、edit、detail routes。
  overview 在在线时每 30 秒刷新，并以状态、6h uptime、五分钟连续性格带和最近检查
  组成 uptime roster；detail status 每 15 秒、history 每 30 秒刷新。
- 详情页展示 capture/quality 横幅、状态、availability/coverage/latency 图、
  Observer 矩阵和 ad-hoc 操作。远端 ICMP capability 未知时显示 `Unknown`，不会把
  本机自检结果投射到远端节点。

## Deployment and migration

- systemd、OpenRC 与 single-image Docker/Compose 都运行相同 binary，不需要额外
  monitor sidecar、listener 或外部 ping command。
- Linux PING 需要内核允许 ICMP datagram socket，或运行身份拥有同一 ICMP 语义的
  raw socket capability；无法满足时只隐藏 ICMP capability 并记录 unsupported，
  HTTP/HTTPS/TCPING 不受影响。
- host-managed upgrade 与 Docker volume 都必须保留 `${XP_DATA_DIR}/uptime.sqlite3`
  和 Repository 节点的 `${XP_DATA_DIR}/history.sqlite3`。旧 uptime SQLite 的 broad
  `(monitor, revision, observer, slot, ad_hoc)` unique key 会自动迁移为仅对 scheduled
  slot 幂等，以保留同一秒的多个 ad-hoc run。

## References

- `./SPEC.md`
- `./HISTORY.md`
- `./contracts/api.md`
