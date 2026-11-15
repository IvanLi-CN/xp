# ADR 0035：xp-proxy 与 Xray 的节点级互斥运行时

## 状态

已接受并由本实现落地。

## 决策

- host-managed systemd 和 OpenRC 节点默认运行 Xray；安装 xp-proxy 二进制或
  service asset 不改变流量归属。
- 每个节点只能选择一个入口运行时：Xray runtime 或 xp-proxy runtime。
  两个运行时同时运行属于部署失败。
- 运行时选择由本地 `xp-ops runtime switch --to xray|xp-proxy --yes` 管理，并持久化在
  `/etc/xp/runtime.json`；缺失状态按 Xray active 解释。
- 切换固定为：停止 XP，停止并确认 Xray 与 xp-proxy，更新 XP 环境，启动目标运行时，
  确认 ready，再启动 XP。
- 任一步失败都停止 XP、Xray 和 xp-proxy，并记录 failed 状态；跨运行时恢复只能由
  下一次显式 switch 执行。
- XP 只通过受保护 control socket 发布 snapshot；xp-proxy 不读取 Raft、SQLite 或 Web
  状态。
- Docker/Compose 在拥有等价服务监督、snapshot persistence、owner handoff、readiness、
  升级和回滚证据前，不广告 xp-proxy capability。

## 取舍

节点级互斥带来有界切换中断，但保证单一入口 owner、可审计的生命周期和不会发生
双进程抢占端口。未来若需要 endpoint 级混合运行，必须另立 ADR，不能放宽本决策。
