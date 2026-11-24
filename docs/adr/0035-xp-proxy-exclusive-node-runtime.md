# ADR 0035：xp-proxy 与 Xray 的节点级互斥运行时

## 状态

已接受并由本实现落地。

## 决策

- 新建 host-managed systemd 和 OpenRC 部署默认选择 `xp-proxy`；已选择运行时的节点
  （包括缺省状态下的 legacy Xray）在 deploy 和 upgrade 中保留原选择。安装 xp-proxy
  二进制或 service asset 本身不改变流量归属。
- 新建部署的首次选择写入 `/etc/xp-ops/deploy/runtime-selection.json`；激活窗口在启动服务前把
  `/etc/xp/runtime.json` 记为 `switching`（XP 只在 `switching`/`active` 下发布 native
  snapshot），真正 ready 后才写 `active`，失败写 `failed`，不伪造 `active`。
- 每个节点只能选择一个入口运行时：Xray runtime 或 xp-proxy runtime。
  两个运行时同时运行属于部署失败。
- 运行时选择由本地 `xp-ops runtime switch --to xray|xp-proxy --yes` 管理，并持久化在
  `/etc/xp/runtime.json`；缺失时若有 deploy 的 runtime-selection 记录则沿用，否则仍按
  Xray active 解释（legacy 节点无该记录）。
- 切换固定为：停止 XP，停止并确认 Xray 与 xp-proxy，更新 XP 环境，启动目标运行时，
  确认 ready，再启动 XP。切换同时收敛 boot enablement：启用目标运行时并禁用另一个。
- 任一步失败都停止 XP、Xray 和 xp-proxy，并记录 failed 状态；跨运行时恢复只能由
  下一次显式 switch 执行。失败时不得留下会在重启后静默启动另一 runtime 的 boot 路径。
- `xp-ops deploy`、`xp-ops runtime switch` 和 `xp-ops upgrade` 都按持久化选择收敛
  systemd/OpenRC boot enablement；upgrade 不改变所选 runtime。
- XP 只通过受保护 control socket 发布 snapshot；xp-proxy 不读取 Raft、SQLite 或 Web
  状态。
- Docker/Compose 在拥有等价服务监督、snapshot persistence、owner handoff、readiness、
  升级和回滚证据前，不广告 xp-proxy capability。

## 取舍

节点级互斥带来有界切换中断，但保证单一入口 owner、可审计的生命周期和不会发生
双进程抢占端口。未来若需要 endpoint 级混合运行，必须另立 ADR，不能放宽本决策。
