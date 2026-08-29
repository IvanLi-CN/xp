# Reality Mesh 反向中继决策历史

## Decisions

- 反向中继解决的是控制面单向失联，不是通用网络隧道。
- Rendezvous 采用双候选确定性分配；运行态连接和 drain 不进 Raft。
- H2C 使用 reqwest SOCKS5、Hyper HTTP/2 prior knowledge 和 Axum 原生接收；不手写协议帧。
- Xray 只使用上游动态 Handler/Routing API。静态 `app.reverse` portal/bridge 槽位不使用，因为它们会与 VLESS inbound 动态 reverse handler 冲突，且无法提供按 generation 的 drain 证明。
- 旧 worker 的回收边界由受控 Xray 重启封顶，而不是维护 Xray fork。
- Cloudflare Tunnel 只属于 Public path，不承载 Reverse underlay。
- 固定版 Xray spike `20260819_102353_be14b3bf_reverse` 在共享测试机证明了两台 Xray 经
  Vision TCP + Reality、XHTTP + Reality 建立动态 VLESS Reverse，并完成受限 SOCKS5、原生
  H2C、精确 origin、unmatched block 与移除隔离。由于测试运行在容器外，SOCKS 仅通过 host
  loopback 映射验证；生产仍使用 `127.0.0.1:10086`，不新增公网监听。该证据不覆盖非对称
  防火墙、signed health、fresh join、部署重启和内存门禁。
- fresh join 已接入 additive `reverse_mesh_bootstrap`：在能力 barrier 和候选可用时，leader
  先写入短期 learner assignment，join 客户端把公开端点、epoch、generation 写入现有 0600
  bootstrap marker；不满足条件时不建立 epoch，继续使用既有 Direct/Public bootstrap。
- assignment worker 对每个 target 发起 reverse-only signed health probe；健康结果只作为短期
  运行态证据，Xray/portal gate 或 probe 失败不会影响 Direct/Public 和成员资格。
- remote Rendezvous 的 outer request 先使用 Reality Mesh，再按既有安全重试语义退回 Public/API；
  caller 与 Rendezvous 为同一节点时使用签名 XP loopback portal，以支持两 voter degraded
  拓扑而不依赖公网回环。primary 与 standby 分别完成 signed health，避免故障切换时才首次验证。
- bootstrap 期间使用独立 `ReverseRole::Bootstrap` 派生域；join operation 完成后双方切回正式
  Primary/Standby，旧 bootstrap worker 进入 120 秒 drain。
- Reverse Assignment 与 target-side Reverse Link 分离：assignment 仍是 durable topology，Link
  Lease、probe 和 circuit 仅为目标节点内存态。ADR 0005 将未验证 Link 固定为初始 probe、一次
  30 秒 recheck、15 分钟 cooldown；健康 replacement 才开始旧 Link 的 120 秒 drain。
  `XP_REVERSE_MESH_ENABLED=false` 是不改 Raft 的本地 fail-closed 回退。
- Xray route reconcile 以 `ListRule` 的当前 tag 集合为幂等依据；仅对同一 desired tag 的
  `app/router: duplicate ruleTag` 保留兼容 fallback。

## Supersession

本主题扩展 `56dtr-reality-fallback-control-plane-mesh` 的 Direct/Public 控制面合同；不复活已 retired 的 `nbs5f-xray-control-plane-relay`。
