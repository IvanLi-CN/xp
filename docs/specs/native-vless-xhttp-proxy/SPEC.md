# Rust 原生 VLESS + REALITY + XHTTP 数据面

> 本文是当前有效的主题合同。实现覆盖、分阶段交付和设计审查边界见
> [IMPLEMENTATION.md](./IMPLEMENTATION.md)，兼容性证据矩阵见
> [COMPATIBILITY.md](./COMPATIBILITY.md)，演进记录见 [HISTORY.md](./HISTORY.md)。

## Related ADRs

- [ADR 0031：Rust 原生 VLESS XHTTP 数据面](../../adr/0031-native-vless-xhttp-data-plane.md)

## Context and Scope

XP 的控制面继续拥有 Raft、节点、endpoint、用户和成员关系的权威状态。
独立的 Rust `xp-proxy` 只负责本机代理数据面：公开 listener、VLESS/REALITY/XHTTP 协议、
固定 fallback、连接生命周期和运行时指标。
两者通过受权限保护的本机 control socket 交换完整的 Proxy Snapshot；
`xp-proxy` 不读取 Raft、SQLite 或 Web 状态。

本主题锁定以下首个生产协议范围：

- TCP-only VLESS，`encryption: none`。
- REALITY client/server，按锁定的 Xray/Mihomo Compatibility Baseline 验收。
- HTTP/2 上的 XHTTP `stream-one`，XP 固定 path `/xp-xhttp`。
- XP 现有六项 XMUX 设置：`max-connections`、`max-concurrency`、
  `c-max-reuse-times`、`h-max-request-times`、`h-max-reusable-secs`、
  `h-keep-alive-period`。
- XP 配置的固定 HTTP、HTTPS、HTTP/2 和 h2c fallback；不允许任意 URL、任意 Host
  或任意 authority 转发。

本主题首先覆盖 host-managed systemd 和 OpenRC 节点。
容器仍是一级部署形态，但在 `xp-proxy` 具有同等服务、就绪、升级和回滚合同前，
不得广告原生数据面 capability，不能把“容器里有二进制”当作功能支持。

## Goals and Non-goals

### Goals

- 将 XP 控制面与 `xp-proxy` 数据面隔离为两个独立 OS 进程和发布边界。
- 用显式 capability 和显式 endpoint ingress owner 控制 native/Xray 选择。
- 定义完整的 snapshot、generation、prepare、activate、drain、rollback、restart 和
  status 合同。
- 在单端口切换时保持唯一 listener owner，失败时 fail closed，禁止双重绑定和
  隐式流量切换。
- 把系统服务安装、就绪检测、升级、回滚、崩溃恢复和兼容性证据纳入同一发布门禁。

### Non-goals

- `xp-proxy` 不直接参与 Raft、Web/API mutation、SQLite 或集群成员变更。
- 不在同一 endpoint 端口同时运行 Xray 和 native listener，也不使用 `SO_REUSEPORT`
  掩盖 owner 冲突。
- 不承诺零中断切换；单端口 handoff 必须有界、可观测并在失败时 fail closed。
- 首个合同不包含 UDP/XUDP、其他 XHTTP mode、sing-mux、Native Reverse 或未验证的
  Xray/Mihomo 字段。
- 不在本轮设计锁定中部署 us3、切换生产流量、发布 release 或合并代码。

## Domain Boundary

### Control-plane authority

XP 是唯一的 endpoint/user 权威。XP 生成的 snapshot 必须完整描述本机 native 数据面所需
的 listener、用户、REALITY 参数、XHTTP 参数、XMUX 参数和 fallback。
`xp-proxy` 不得从上一次本地运行状态推断缺失字段，也不得自行修改用户或 endpoint 权限。

### Data-plane ownership

每个 VLESS XHTTP endpoint 必须明确记录 ingress owner：`xray` 或 `native`。历史 metadata
缺少该字段时继续视为 `xray`，避免仅因升级而改变既有流量归属；`transport: xhttp` 只
描述协议，不得隐式表示 native。
只有 owner 为 `native` 且本节点通过 capability 和运行时就绪检查时，XP 才能把该 endpoint
放入 native snapshot，并从 Xray desired config 中排除同一端口。

### Usable ingress point

一个 native endpoint 只有同时满足下列条件才算 usable ingress point：

1. endpoint 的 ingress owner 是显式的 `native`，不是二进制存在、环境变量存在或
   `transport: xhttp` 的推断结果。
2. 本节点已广告专用 native capability，且 capability 的本地服务、control socket、协议
   版本和自测均通过。
3. XP 已为当前 snapshot 取得有效的持久 generation，`xp-proxy` 已接受同一 generation，
   所有该 endpoint listener 均成功绑定并报告 `active`。
4. Xray、XP 管理 listener、Tunnel 或其他已登记 owner 不再持有同一端口；没有 pending
   cutover、drain、rollback、restart 或未知结果。
5. native 入站握手、自身转发和固定 fallback 的本地 self-test 通过，且该发布候选拥有
   Compatibility Baseline 的证据。
6. status 对外报告 owner、active generation 和 ready/active 结论，未隐藏任何 bind、
   capability、snapshot 或 persistence 错误。

仅安装 `xp-proxy`、进程存活、control socket 可连接、snapshot 已 prepare、Xray 仍占用
端口，或 capability 字符串被静态写入，均不构成 usable ingress point。没有 native endpoint
时，服务可以处于 ready/idle，但节点不能声称存在 native ingress。

## Requirements

### Runtime and capability

- **REQ-BOUNDARY:** `xp` 和 `xp-proxy` 必须是独立 OS 进程；XP 不得通过子进程 supervisor
  代替 init system 拥有 `xp-proxy` 的 restart、stop 或升级生命周期。
- **REQ-CAPABILITY:** native capability 必须是独立、可验证、按节点声明的 capability。只有
  独立服务已安装、control socket 权限正确、协议版本兼容、loopback self-test 通过且当前
  运行时可被服务管理器确认 ready 后才可广告。安装资产不能自动启用 capability。
- **REQ-OWNER:** endpoint 必须有显式 owner marker，取值为 `xray` 或 `native`。旧 metadata
  缺失 marker 时按 `xray` 解释；owner mutation 需要条件快照，不能被旧 Web/API 请求静默覆盖。
- **REQ-INGRESS-READY:** 对外 usable 的判定必须满足本文件的六项条件；任何一项未知或
  失败都报告 not-ready/failed，并保持端口唯一 owner。

### Snapshot and generation

- **REQ-SNAPSHOT:** XP 发送完整、原子的、带 schema version 的 Proxy Snapshot。prepare、
  activate、drain、stop 和 rollback 都必须以 snapshot/generation 作为边界；`xp-proxy`
  不得接受只包含局部 listener 或用户增量的生产请求。
- **REQ-EMPTY-SNAPSHOT:** 没有 native endpoint 时允许 XP 发送零 listener 的完整 snapshot，
  用于释放旧 listener 并声明 native idle；零 listener 不得被报告为 active ingress。
- **REQ-GENERATION:** generation 必须是 XP 管理的、按节点单调递增且持久化的 sequence，
  在 XP 重启、`xp-proxy` 重启和服务升级后仍能比较新旧 snapshot。进程内计数器、时间戳或
  重新启动后从 1 开始的值不能作为生产 generation。
- **REQ-GENERATION-ROLLBACK:** 普通 prepare/activate 不能接受低于 active generation 的
  snapshot。回滚必须是带 `expected_active_generation`、目标 snapshot 和明确 rollback
  intent 的显式操作；目标校验失败时不得降级为旧 listener。
- **REQ-PERSISTENCE:** `xp-proxy` 可以持久化最近一次成功 snapshot 供诊断和校验，但重启
  后不得仅凭陈旧本地 snapshot 对外接收新连接；必须等 XP 重放并确认当前 generation。
  持久化文件损坏、权限错误或版本不兼容时 fail closed。

### Control protocol and lifecycle

- **REQ-CONTROL:** control protocol 必须版本化，并在每个 request/response 中绑定
  `protocol_version`、`request_id`、snapshot/generation 和结构化 error。协议版本、snapshot
  schema version 和 generation 是不同概念，不能互相替代。
- **REQ-IDEMPOTENCY:** 对相同 request identity 和 snapshot digest 的重试必须可判定为同一
  次操作；超时后 XP 必须先通过 status 查询结果，再决定重试或回滚。未知结果不能直接当作
  failed 并发布另一份 snapshot。
- **REQ-LIFECYCLE:** 生产状态至少覆盖 `idle`、`preparing`、`ready`、`active`、`draining`、
  `stopped`、`failed` 和 `rollback_required`。失败 generation 不能成为 active；`ready`
  表示候选已验证，不表示公开端口已归 native 所有。
- **REQ-LISTENER:** activate 只有在旧 owner 已释放目标端口、候选 snapshot 校验通过、
  listener 全部绑定且 self-test 成功后才能返回 active。listener 绑定部分成功时必须撤销
  本次候选的全部 listener，不得留下半活跃 snapshot。
- **REQ-STATUS:** status 必须报告 active/prepared generation、owner、listener bind 状态、
  lifecycle、最近错误、连接计数和 capability readiness，但不得暴露 UUID、私钥或完整
  snapshot secret。

### Xray/native switching and rollback

- **REQ-SINGLE-OWNER:** 一个 endpoint port 在任一时刻只能有一个 owner。XP 在 Xray desired
  config 和 native snapshot 中必须使用同一份 owner 事实；发现冲突时拒绝 mutation/reconcile，
  不得依靠启动顺序或 bind race 解决。
- **REQ-SWITCH-TO-NATIVE:** Xray -> native 必须按以下顺序完成：验证 endpoint owner、capability、
  port 和旧 generation；prepare native candidate；让 Xray 删除该 endpoint 并确认端口释放；
  activate native；确认 active generation 和握手 self-test；最后提交 owner 已切换。任一步失败，
  native 不得接受新连接。
- **REQ-SWITCH-TO-XRAY:** native -> Xray 必须先停止新 native session，进入 drain 并等待有界
  deadline；确认 native listener 释放端口后再写入/启动 Xray inbound，并确认 Xray ready；成功后
  才提交 owner 为 xray。
- **REQ-ROLLBACK-ORDER:** 切换或升级失败时，回滚遵守“先停止当前 owner、确认端口释放、再恢复
  目标 owner、最后确认 ready”的顺序。恢复目标不能 ready 时，endpoint 保持 stopped/failed，
  不得同时启动另一 owner 争抢同一端口。
- **REQ-NO-AUTO-FAILBACK:** native 进程崩溃、control socket 消失、健康检查超时或升级结果未知，
  不得自动把同一 endpoint 交给 Xray。服务管理器可以重启 `xp-proxy`；owner 变更和跨数据面的
  rollback 必须由 XP 的显式协调操作完成。

### Protocol and fallback contract

- **REQ-VLESS:** 入站和出站只承诺 TCP stream 与 `encryption: none` VLESS；用户认证失败、
  framing 错误或非法 destination 必须 fail closed。
- **REQ-REALITY:** REALITY server/client 必须覆盖锁定基线中的 key schedule、SNI、ALPN、
  fingerprint、short ID、TLS record 和 authenticated/unauthenticated 分流；不兼容握手不得
  被当作 authenticated session。
- **REQ-XHTTP:** XHTTP 必须使用 HTTP/2 `stream-one` 和固定 `/xp-xhttp` 合同。一个 H2 transport
  可承载多条已验证 stream，但 cancellation、flow control、disconnect 和 reconnect 必须有明确
  session 结果。
- **REQ-XMUX:** 六项 XMUX 设置必须与既有 endpoint schema、订阅渲染和 Mihomo 兼容合同一致；
  不得把未实现的连接复用参数静默接受为有效设置。
- **REQ-FALLBACK:** fallback 只能指向 XP 生成的固定 target 和 exact authority allowlist，支持
  HTTP、HTTPS、HTTP/2、h2c 的明确协议语义。实现必须按 scheme 处理或拒绝该 scheme，不能把
  四种 scheme 静默当成同一种 raw TCP relay；HTTP-aware 路径必须清理 hop-by-hop headers。
- **REQ-FALLBACK-ISOLATION:** fallback 不得使用客户端提供的任意 Host、SNI、URL、路径或
  authority 扩大目标集合；目标解析、端口和协议必须在 snapshot 校验时绑定，并记录拒绝原因。

### Deployment, upgrade and security

- **REQ-HOST-SERVICE:** systemd/OpenRC 必须拥有独立的 `xp-proxy` service asset、socket
  目录/权限、启动停止、ready 检查和两次连续 OpenRC ready 观测。一个异步命令返回 0 不足以
  证明服务 ready。
- **REQ-UPGRADE:** host-managed upgrade 必须把 `xp-proxy` 纳入同一 transaction 的 binary
  backup、停止/drain、替换、ready、diagnostics、cleanup 和 rollback 合同。升级前不得留下
  旧 proxy 监听；升级后只有所有运行时 ready 且 XP 已重新确认 snapshot 才能报告成功。
- **REQ-UPGRADE-ROLLBACK:** rollback 必须恢复 XP、Xray、cloudflared 和 xp-proxy 的匹配版本及
  service assets；旧 native runtime 或 snapshot 无法 ready 时保留 endpoint stopped，并遵守
  既有 `rollback_failed`、事务 backup 和 `cleanup_incomplete` 保护。
- **REQ-CONTAINER-BOUNDARY:** Docker/Compose 在没有等价的独立 service、snapshot persistence、
  owner handoff、upgrade/rollback 和 readiness evidence 前不得宣称 native capability；容器
  继续运行当前受支持的 Xray data plane。
- **REQ-SECURITY:** control socket、snapshot persistence、service user、private key、UUID 和
  日志必须遵守最小权限；socket 替换不得跟随 symlink；错误和 metrics 不得泄露 credential 或
  private key。
- **REQ-RESOURCE:** native data plane 必须在 Compatibility Baseline 与部署资源预算内运行；
  listener、H2 stream、XMUX、fallback、日志和 status 必须有界，不能用无界 task、连接池或
  request body 读取掩盖内存/FD 增长。
- **REQ-OBSERVABILITY:** system status、service status 和 control status 必须区分
  `installed`、`process_ready`、`snapshot_ready`、`active_ingress`、`draining`、`failed` 和
  `outcome_unknown`，并显示当前 owner 与 generation。
- **REQ-FUTURE-CAPABILITY:** UDP/XUDP、其他 XHTTP mode、额外 Xray 字段、Native Reverse 或
  任何新 fallback 行为，必须先增加独立 capability、fixture、互操作证据和本主题版本更新。

## Compatibility and Acceptance

生产候选必须同时提供以下四条路径的证据：

```text
Rust inbound  <- Xray outbound
Rust inbound  <- Mihomo client
Rust outbound -> Xray inbound
Rust outbound -> Mihomo-compatible server
```

每条路径都必须覆盖小包、大包、并发 stream、idle reuse、主动断开、reconnect、错误凭据、
畸形 REALITY/HTTP/HTTP2 输入、fallback authority rejection 和资源上界。固定基线为 Xray
`26.3.27` 与 Mihomo `v1.19.29`；未锁定版本不得替代基线证据。

## Verification

- **VER-BOUNDARY** covers: REQ-BOUNDARY, REQ-HOST-SERVICE. XP 与 proxy 是独立服务；
  systemd/OpenRC 可独立停止、启动、检查 ready。
- **VER-CAPABILITY** covers: REQ-CAPABILITY, REQ-OWNER, REQ-INGRESS-READY.
  资产存在、进程存活、socket 可用但未完成 snapshot 时均不会虚报 ingress。
- **VER-SNAPSHOT** covers: REQ-SNAPSHOT, REQ-EMPTY-SNAPSHOT, REQ-PERSISTENCE.
  完整 snapshot、零 listener、损坏恢复、XP 重放和 fail-closed。
- **VER-GENERATION** covers: REQ-GENERATION, REQ-GENERATION-ROLLBACK.
  XP 重启/代理重启/重复请求/显式 rollback 的顺序与冲突处理。
- **VER-CONTROL** covers: REQ-CONTROL, REQ-IDEMPOTENCY, REQ-LIFECYCLE, REQ-STATUS. framing、版本错误、unknown outcome、结构化错误和 status 一致性。
- **VER-OWNER** covers: REQ-LISTENER, REQ-SINGLE-OWNER, REQ-SWITCH-TO-NATIVE, REQ-SWITCH-TO-XRAY. 相同端口绝不双绑；两方向 handoff 的端口释放和失败收敛。
- **VER-ROLLBACK** covers: REQ-ROLLBACK-ORDER, REQ-NO-AUTO-FAILBACK. bind 失败、self-test 失败、进程崩溃、旧版本恢复失败时均无隐式跨 owner 切换。
- **VER-PROTOCOL** covers: REQ-VLESS, REQ-REALITY, REQ-XHTTP, REQ-XMUX. Rust 与 Xray/Mihomo 双向 payload、stream-one、复用和重连。
- **VER-FALLBACK** covers: REQ-FALLBACK, REQ-FALLBACK-ISOLATION. 四种 scheme 的真实语义、header 清理、exact authority 和拒绝路径。
- **VER-DEPLOY** covers: REQ-UPGRADE, REQ-UPGRADE-ROLLBACK, REQ-CONTAINER-BOUNDARY. Agent VM 中 systemd/OpenRC 安装、升级、回滚、崩溃恢复和容器 capability 边界。
- **VER-SECURITY** covers: REQ-SECURITY. socket/persistence 权限、symlink 防护、secret 日志审计和非法输入 fail closed。
- **VER-RESOURCE** covers: REQ-RESOURCE, REQ-OBSERVABILITY. H2/连接/FD/内存/日志有界，状态分类可解释且不把 unknown 当 zero/ready。
- **VER-FUTURE** covers: REQ-FUTURE-CAPABILITY. 未授权协议字段或 mode 被拒绝，新增 capability 未经 fixture 不能启用。

## Acceptance Criteria

- **AC-1 Usable ingress:** 只有本文件“Usable ingress point”六项条件全部成立时，系统才报告
  `active_ingress`；二进制安装、服务存活或 prepare 单独均失败。
- **AC-2 Unique owner:** 任意切换、升级、重启和回滚都能证明目标端口先由旧 owner 释放，
  再由新 owner 绑定；冲突会停止并留下可诊断状态。
- **AC-3 Durable ordering:** XP 崩溃或 proxy 重启不会重置 generation，也不会让陈旧 snapshot
  越过 XP 的当前提交；低 generation 只能走显式 rollback。
- **AC-4 Fail closed:** control socket、snapshot、私钥、版本、self-test、fallback 或服务
  ready 任一校验失败时，不接收新的 native session，不自动启动 Xray 抢占同一端口。
- **AC-5 Protocol evidence:** Xray `26.3.27` 和 Mihomo `v1.19.29` 四条路径均通过基础流量、
  复用、断线重连、错误输入和 fallback 测试。
- **AC-6 Host lifecycle:** systemd 与 OpenRC 都能独立安装、启动、停止、drain、升级、回滚，
  且就绪检查不是异步退出码的假阳性；OpenRC 需要两次连续 ready/stopped 观测。
- **AC-7 No scope leak:** 未通过同等合同的 Docker/Compose、UDP/XUDP、Native Reverse、未来
  protocol fields 和未锁定版本不会获得 native production capability。
- **AC-8 Evidence completeness:** 交付报告同时包含代码单测、Agent VM service evidence、
  Xray/Mihomo interoperability evidence、资源/安全结果和明确的失败项；任何未知结果均
  保留为未完成而非降级成通过。

## References

- `crates/xp-proxy-core/src/control.rs`：当前控制协议和 generation 状态机实现。
- `crates/xp-proxy/src/main.rs`：当前 listener、fallback 和 control socket 实现。
- `src/native_proxy_supervisor.rs`：当前 XP snapshot 发布与临时子进程 supervisor。
- `src/reconcile.rs`：当前 Xray reconcile 后的 native snapshot 发布位置。
- `src/ops/upgrade/managed_runtimes.rs`：当前 xp-proxy asset 安装与 host runtime upgrade 边界。
- `src/ops/runtime_activation.rs`：systemd/OpenRC readiness 语义。
- `scripts/e2e/run-local-xray-e2e.sh`：现有 Xray/Mihomo E2E 入口。
