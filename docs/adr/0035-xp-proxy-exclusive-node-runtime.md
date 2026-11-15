# ADR 0035：xp-proxy 与 Xray 的节点级互斥运行时

## 状态

待设计审查。本文记录已确认的目标设计，不代表实现、部署或生产切换已经完成。

## 背景

XP 发布物已经包含 `xp-proxy` 二进制，但“文件已发布”“服务已安装”“服务已就绪”和“入口已由 `xp-proxy`
承载”此前没有被清楚区分。现有实现还允许安装资产而不建立完整的服务生命周期，导致升级结果无法证明 `xp-proxy` 是否真正可用。

本项目要求同一节点不能同时使用 Xray 和 `xp-proxy` 承载入口流量。这个约束属于节点内部运行时编排，不是 Raft 的集群模式，也不是发布包的属性。

## 决策

- 首发边界是 host-managed systemd 和 OpenRC。Docker/Compose 在具备等价的独立服务、状态持久化、readiness、升级、回滚和流量切换证据前，不宣称支持
  `xp-proxy` 入口。
- 节点本地只能选择一个入口运行时：Xray runtime 或 `xp-proxy` runtime。选中的运行时可以运行，另一个必须停止；两者同时运行是部署失败。
- 运行时选择由本节点的 `xp-ops` 显式命令管理，并持久化在节点本地。Raft 不保存运行时选择；XP 只通过 control socket 发布 snapshot，并向集群报告可验证的
  capability、owner 和健康结果。
- 既有节点升级默认保持当前 Xray runtime，不因安装新二进制或 service asset 自动切换流量。
- 切换到 `xp-proxy` 的顺序固定为：停止 XP、停止并确认 Xray、确认端口释放、启动并确认 `xp-proxy`、发布完整 snapshot、确认
  generation、listener、self-test 和公网入口，再启动 XP。
- 切换失败时两个运行时保持停止，记录结构化失败状态；系统只允许重启同一个失败运行时。切回 Xray 必须由显式 rollback 操作完成，不允许自动跨运行时切换。
- `xp-proxy` service asset、独立 service identity、control socket、ready/stopped 观测、升级备份、回滚和清理属于同一个
  host-managed 生命周期合同。仅安装二进制不能作为升级成功证据。

## 运行时状态

运行时选择和入口可用性是两个不同事实：

- `installed`：目标二进制和 service asset 已安装。
- `stopped`：服务未运行且没有 listener。
- `ready`：选中的服务已由 systemd/OpenRC 确认运行，control socket 和基础协议检查通过。
- `active_ingress`：XP 已确认当前 snapshot/generation、所有 listener、端口唯一 owner、self-test 和兼容性证据。

没有 native endpoint 时，`xp-proxy` 仍可以在选择它作为本地运行时时保持 ready，但不得报告 `active_ingress`。在 Xray runtime
下，`xp-proxy` 必须停止，不能以 idle 方式并行运行。

## 升级与回滚

升级必须将 XP、Xray、cloudflared 和 `xp-proxy` 纳入同一个受保护事务。升级前读取本地运行时选择、当前 owner 和 active generation；替换二进制与
service asset 后，只启动选择的运行时，并在 XP 重放 snapshot 后重新确认 owner 和
generation。任一服务、socket、snapshot、listener、self-test 或清理失败都进入既有 rollback contract；无法证明恢复的运行时 ready
时，入口保持停止。

## 发布门禁

正式发布必须同时提供：systemd 和 OpenRC 的安装/启动/停止/升级/回滚证据，Xray 与 `xp-proxy` 的互斥进程证据，control socket
权限，snapshot/generation 恢复，真实 XHTTP 流量，故障 fail-closed，以及资源和安全证据。发布包中存在 `xp-proxy` 文件本身不满足这些门禁。

## 取舍

节点级互斥会带来明确的切换中断，并禁止同一节点混合承载不同 endpoint。这个代价换取了单一入口运行时、可审计的升级边界和不会发生双 owner 监听。未来如果需要 endpoint
级混合运行，必须另立设计，不能在本决策上隐式放宽。
