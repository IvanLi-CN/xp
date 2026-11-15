# ADR 0031：Rust 原生 VLESS XHTTP 数据面

## 状态

高层架构已采纳；生产切换与 host-managed 生命周期细节在主人确认前不视为已授权实现。

## 背景

XP 当前由控制面管理节点、endpoint、用户和 Xray 配置。为了把协议实现、资源边界和升级
故障从控制面隔离，需要一个独立的 Rust `xp-proxy` 数据面。单独增加一个 binary 仍会共享
XP 的进程生命周期，不能提供清晰的服务 ready、端口 owner 和回滚边界；因此必须把它作为
独立 OS runtime 设计。

## 决定

- XP 继续拥有 Raft、endpoint/user 权威状态，并生成完整、版本化的 Proxy Snapshot。
- `xp-proxy` 作为独立 OS process/service 拥有 listener、VLESS/REALITY/XHTTP、固定
  fallback、连接生命周期和运行时指标；它不读取 XP Raft、SQLite 或 Web 状态。
- endpoint 的 `transport=xhttp` 不自动选择 native。每个 endpoint 必须有显式 ingress
  owner；旧 metadata 缺失 owner 时保持 Xray。
- 一个端口只能有一个 owner。Xray -> native 与 native -> Xray 都必须先停止并确认旧
  owner 释放端口，再启动新 owner；不使用 `SO_REUSEPORT`、bind race 或隐式自动 failback。
- native capability 只有在独立 service、control socket、协议版本、loopback self-test、
  当前 snapshot generation 和 listener readiness 都通过后才能广告。安装 binary、进程存活
  或 prepare 不等于 usable ingress。
- generation 由 XP 持久化并按节点单调递增；陈旧本地 snapshot 在 proxy 重启后不能直接
  对外服务。低 generation 只能通过带 expected active generation 的显式 rollback。
- 首个协议合同锁定 TCP-only VLESS、REALITY、XHTTP `stream-one`、六项 XMUX 和固定
  HTTP/HTTPS/H2/h2c fallback；UDP/XUDP、其他 XHTTP mode、Native Reverse 和未验证字段
  需要独立 capability 与互操作证据。
- host-managed systemd/OpenRC 是首个实现边界。Docker/Compose 直到具备同等 service、
  readiness、upgrade、rollback 和 owner handoff evidence 前，不得广告 native capability。

## 切换与回滚边界

切换是由 XP 协调的有界 operation：prepare candidate、释放旧 owner、activate 新 owner、
验证 active generation/self-test、最后提交 owner。任何中间失败都进入 failed 或
rollback_required；无法恢复目标 owner 时 endpoint 保持 stopped，不启动两个数据面争抢端口。
proxy 崩溃或 control socket 消失只允许服务管理器重启同一 proxy；跨到 Xray 的 rollback
必须是明确的 owner operation。

## 兼容性与发布门禁

生产候选必须通过锁定的 Xray `26.3.27` 和 Mihomo `v1.19.29` 四条双向路径、fallback、
资源、安全和 systemd/OpenRC Agent VM evidence。当前 `xp-proxy` spike、release asset 或
Xray/Mihomo 单向 smoke 不足以解锁 production capability。

## 未决取舍

service identity、generation 的具体持久介质、capability/metadata 的精确名称、单端口
中断与 drain deadline、四种 fallback 的协议语义、native 崩溃后的自动 rollback，以及
Docker 首发范围仍需主人确认。它们记录在 [native proxy implementation design](../specs/native-vless-xhttp-proxy/IMPLEMENTATION.md)，
确认前不应由实现者自行假定。

## 后果

- 需要独立 service assets、control protocol、snapshot persistence、owner handoff、
  systemd/OpenRC readiness 和升级/回滚协调。
- 现有 Xray data plane 在 owner 未显式切换前继续承担流量，native binary 的安装不会改变
  生产行为。
- 互操作性和部署故障变成发布门禁，代价是 native capability 不能仅凭编译通过或进程启动
  就启用。
