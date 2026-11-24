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

运行时选择是节点本地状态，不进入 Raft。新建 host-managed 部署默认选择 `xp-proxy`；已有节点保留
其已选 runtime（含缺省状态下的 legacy Xray），deploy 和 upgrade 都不会隐式翻转。只有
`xp-ops runtime switch --to xray|xp-proxy --yes` 可以改变既有节点的选择。
本地 `/etc/xp/runtime.json` 缺失时，若存在 deploy 的 runtime-selection 记录则沿用该选择，
否则仍按 Xray active 解释；legacy 节点没有该记录，因此不会被翻转。新建部署先把选择写入
`/etc/xp-ops/deploy/runtime-selection.json`，激活窗口在启动服务前将 `/etc/xp/runtime.json`
记为 `switching`（XP 只在 `switching`/`active` 下发布 snapshot），ready 后才写 `active`，
不用伪造 active 状态。切换中或失败状态禁止 XP 继续提供入口流量。
仅有 deploy 选择记录而没有 `/etc/xp/runtime.json` 的节点视为未激活：
`xp-ops runtime status` 报告 `not_activated`，`xp-ops upgrade` 拒绝执行，必须先完成部署激活
或显式 runtime switch。
节点启动引导（systemd/OpenRC）必须只启用所选 runtime 并禁用另一个；deploy、runtime switch
和 upgrade 都按持久化选择收敛 boot enablement，失败时 fail closed，绝不回退到另一 runtime。

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
- 用显式 capability 和节点级 runtime 选择控制 native/Xray 归属。
- 定义完整的 snapshot、generation、prepare、activate、drain、rollback、restart 和
  status 合同。
- 在单端口切换时保持唯一 listener owner，失败时 fail closed，禁止双重绑定和
  隐式流量切换。
- 把系统服务安装、就绪检测、升级、回滚、崩溃恢复和兼容性证据纳入同一发布门禁。

### Non-goals

- `xp-proxy` 不直接参与 Raft、Web/API mutation、SQLite 或集群成员变更。
- 不在同一 endpoint 端口同时运行 Xray 和 native listener，也不使用 `SO_REUSEPORT`
  掩盖 owner 冲突。
- 不在同一节点同时运行 Xray 和 `xp-proxy` runtime；节点运行时选择属于本地 `xp-ops` 生命周期。
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

宿主节点只运行一种入口模式，由节点级 runtime 状态显式选择 `xray` 或 `xp-proxy`。显式选择 `xp-proxy` 后，本机受支持的 VLESS XHTTP 端点统一进入
native snapshot，并从 Xray desired config 中排除同一端口；不要求逐端点修改或迁移历史 `ingress_owner`。节点未选择 native 时，历史
metadata 缺少该字段仍保持 Xray；`transport: xhttp` 本身不启用 native。native 服务仍须通过 capability 和运行时就绪检查。

### Usable ingress point

一个 native endpoint 只有同时满足下列条件才算 usable ingress point：

1. 节点通过显式 runtime 切换选择 `xp-proxy`，且该端点属于受支持的 XHTTP 入口；不能仅凭二进制存在或 `transport: xhttp` 判断可用。
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
- **REQ-OWNER:** 节点级显式 runtime 状态决定受支持入口的实际 owner；Xray config 与 native snapshot 必须使用同一选择。历史
  endpoint marker 不得阻止已授权的节点切换，也不得由普通 Web/API 更新静默改变节点 runtime。
- **REQ-INGRESS-READY:** 对外 usable 的判定必须满足本文件的六项条件；任何一项未知或
  失败都报告 not-ready/failed，并保持端口唯一 owner。

### TCP 地址族与 wildcard

- **REQ-IP-WILDCARD:** XP 生成的 `0.0.0.0:PORT` 表示自动 wildcard；具备 IPv6 和 IPv4-mapped 能力的 Linux 使用显式
  `IPV6_V6ONLY=false` 的单个双栈 socket，接收 IPv4 与 IPv6。不得依赖系统 `bindv6only` 默认值，也不得使用 `SO_REUSEPORT`。
- **REQ-IP-EXPLICIT:** 非 wildcard 地址只绑定指定接口和地址族；显式 `[::]:PORT` 为 IPv6-only wildcard，不隐式扩大 IPv4 监听范围。
- **REQ-IP-DEGRADE:** 自动 wildcard 仅在内核明确不支持或禁用 IPv6/双栈时退回 IPv4；端口冲突、权限和资源错误必须失败。显式 IPv6
  地址绑定失败不得退回 IPv4。所有所需监听完成后才可激活 generation；候选失败恢复旧 generation 的监听，恢复失败报告停止。
- **REQ-IP-CHAIN:** 入站连接、出站代理远端、本地出站 listener 和 VLESS 最终 TCP 目标均支持 IPv4/IPv6；仍使用系统解析，
  不引入自有 DNS。解析出多个地址时按 250 ms 错峰并发拨号（Happy Eyeballs 语义）：首个成功连接胜出并取消其余
  尝试，单个地址失败立即启动下一个候选，全部失败返回最后一个错误。任何地址族挂起都不得阻塞另一地址族的可用连接
  （例如 AAAA 黑洞时域名代理请求必须继续走 IPv4）。IPv4-mapped peer 按 IPv4 记录。
- **REQ-IP-FALLBACK:** 已配置的 HTTP/HTTPS/H2/h2c 固定 fallback 支持合法的 bracketed IPv6 authority；保留精确
  allowlist、端口、Host、SNI 和既有 header 合同，拒绝畸形 bracket、地址与端口，不增加任意转发。
- **REQ-IP-COMPAT:** 保留 snapshot bind 表达和 control protocol v2，不增加开关，不改 endpoint
  地址、订阅模式、生产网络、NAT、防火墙或 DNS。

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
- **REQ-SWITCH-TO-NATIVE:** Xray -> native 是节点级操作：验证 capability、配置、端口和旧 generation；停止 XP
  及旧运行时并确认释放入口，再启动候选服务，由 XP 发布和激活 snapshot。只有 active generation、listener 和 self-test 均就绪才提交节点
  runtime 为 active；任一步失败不得报告切换成功，不提交逐端点 owner mutation。
- **REQ-SWITCH-TO-XRAY:** native -> Xray 是节点级操作：停止 XP 及 native 运行时，确认 listener 释放后再启动 Xray；确认 Xray
  的实际 API 和入口就绪后恢复 XP 并提交节点 runtime 为 active。
- **REQ-ROLLBACK-ORDER:** 切换或升级失败时，回滚遵守“先停止当前 owner、确认端口释放、再恢复
  目标 owner、最后确认 ready”的顺序。恢复目标不能 ready 时，endpoint 保持 stopped/failed，
  不得同时启动另一 owner 争抢同一端口。
- **REQ-NO-AUTO-FAILBACK:** native 进程崩溃、control socket 消失、健康检查超时或升级结果未知，
  不得自动把同一 endpoint 交给 Xray。服务管理器可以重启 `xp-proxy`；owner 变更和跨数据面的
  rollback 必须由 XP 的显式协调操作完成。

### Protocol and fallback contract

托管默认 XHTTP 端点启用 HTTPS canary 时，XP 发布可选的 `reality_tls_target`。
目标必须是与 `reality.dest` 一致的本机 canary loopback 地址和非零端口，
且不得指回任何接入端口。合法但未通过 REALITY 认证的 TLS ClientHello 和后续数据
透传给该目标，由 canary 终止 TLS 并处理 HTTP 路由。SNI 和 Host 不参与目标选择。
拨号失败关闭连接；有认证的代理流量与明文流量保留原路径。

`canary_upstream` 是 canary 收到 HTTP 请求后的可选上游，不决定 TLS 路由是否存在。
XP 必须在同一 control v2 连接的 `Status` 响应中确认 `reality_tls_fallback_v1`，
才发送含该目标的 `Prepare` 请求。旧 snapshot 仍可读取。
若托管回落目标无效或与本机 canary 不符，XP 记录冲突并拒绝发布新快照。旧代次继续生效。

XP 在快照中写入 `managed_tls_fallback_ready`，状态响应只报告已激活代次的值。

没有托管端点时回落视为就绪。任一托管 XHTTP 端点缺少有效目标时，回落未就绪。

切换或升级须确认快照已激活且回落已就绪。回滚仅允许旧版本省略字段，明确未就绪仍失败。

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

## Performance requirements

详细性能合同见 [contracts/performance.md](./contracts/performance.md)。性能测试与协议互操作、服务生命周期及生产部署验收是不同证据，不能互相替代。

- **REQ-PERF-COMPARE:** 候选 xp-proxy 必须与测试启动时锁定的 main xp-proxy 和 Xray `26.3.27` 在同一 GitHub-hosted
  Linux runner 上串行交替测试，固定可比配置并记录准确版本；不得以历史性能数值替代同场测量。
- **REQ-PERF-COVERAGE:** 自动轻量覆盖入站、小请求延迟、上传/下载和并发 1/8；手动全量增加出站及并发 16/64。

两档必须检查内容完整性、真实错误率、有效吞吐、延迟、CPU、内存和资源释放，不得以提高连接上限替代默认配置的并发正确性。

- **REQ-PERF-TOPOLOGY:** 互操作必须覆盖 8 个独立目标、单客户端 16 条流、四客户端 64 条流，
  混合 IP/域名和快慢/拒绝/重置目标；校验目标身份、payload 隔离及 Linux 实际 1/4 条底层连接。
  慢目标由事件屏障保持阻塞，其他目标必须在释放前完成；失败目标不得破坏正常流。
  相同 fixture 先通过锁定 Xray 服务端，再验证候选，不以该功能短测宣称公网吞吐或长期公平性。

- **REQ-PERF-THRESHOLD:** 有效测量中，候选对应场景的吞吐不得低于 main 与锁定 Xray 各自的 90%，小请求 p50/p95 相对两个对照各自不得增加超过 5 ms；
  合法负载零异常失败、内容校验通过、连接结束后资源释放。

测量不足必须明确非通过，不自动识别特殊缺陷或放宽门槛。

- **REQ-PERF-BUDGET:** 轻量按四分钟、全量按六分钟墙钟设计，性能执行不编译程序；下载、准备、测试和证据上传计入预算，排队与上游构建不计入。
  轻量的轮转重复采样为门禁稳定性所需，不以缩短采样换取更短墙钟。

- 轻量五分钟、全量十分钟安全超时；超过设计预算告警，不直接作为性能数值失败。

- **REQ-PERF-ARTIFACT:** 上游提供独立 xp-proxy 优化构建产物及 SHA、target、构建配置、toolchain 和二进制校验和；消费端必须精确验证来源与候选
  /main 身份，不用其他成功产物补位。
- **REQ-PERF-TRIGGER:** 相关协议代码、共享依赖、锁文件、工具链、构建和性能测试变更必须自动运行轻量测试；main 的相关变更同样自动测试。

无关变更明确报告不适用，必需检查不得永久等待。

- **REQ-PERF-GATE:** 测量结论、PR 合并门禁和人工有效性标记必须分离。

`perf:waive` 只豁免本 PR 的性能合并门禁，不停止测试，不改变原始结果，不影响 main 后续测试或其他 CI。门禁只依据自动轻量及豁免状态，手动全量独立报告。

- **REQ-PERF-ANNOTATION:** 作废与恢复由人经指定工作流明确选择准确 run/attempt 并填写理由，保留完整审计链和原始结果；标记精确影响该次结果，不禁测整个 SHA，不
  由机器推断。

独立 Check 展示须与指定工作流实际生成的操作记录核对，不只凭名称、App 或自报链接认定来源。

- **REQ-PERF-RETENTION:** 测试结果、标记及审计证据统一按 90 天设计并遵守仓库实际留存上限；过期、缺失或审计不完整的结果不得复用，已作废结果不得因标记消失自动恢复为有效。

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

- **VER-PERF-COMPARE** covers: REQ-PERF-COMPARE, REQ-PERF-ARTIFACT. 核对同场三程序测量、固定 main SHA、构建
  manifest、错误 SHA/target/校验和及缺失产物的负例。
- **VER-PERF-MEASURE** covers: REQ-PERF-TOPOLOGY, REQ-PERF-COVERAGE,
  REQ-PERF-THRESHOLD, REQ-PERF-BUDGET. 覆盖双档矩阵、默认并发正确
  性、完整性、双重门槛、资源回收、墙钟明细、预算告警及安全超时，保留无效测量的非通过状态。
- **VER-PERF-AUTOMATION** covers: REQ-PERF-TRIGGER, REQ-PERF-GATE. 验证相关/无关路径、main push、PR 新提交、加标/去标、
  豁免时测量继续、全量不影响自动门禁和旧结果不会解锁新候选。
- **VER-PERF-RECORDS** covers: REQ-PERF-ANNOTATION, REQ-PERF-RETENTION. 验证人工 invalidate/restore、同名伪标
  记拒绝、指定工作流来源、run attempt 隔离、同 SHA 新 run、已作废数据排除、记录过期或读取失败不得自动恢复。

- **VER-IP-SOCKET** covers: REQ-IP-WILDCARD, REQ-IP-EXPLICIT, REQ-IP-DEGRADE.
  验证双栈、显式范围、`bindv6only=1`、禁用 IPv6、端口/权限错误、generation 回滚及 drain/restart。
- **VER-IP-CHAIN** covers: REQ-IP-CHAIN, REQ-IP-FALLBACK, REQ-IP-COMPAT.
  验证四种地址族组合、IPv6 远端、系统 DNS、固定 fallback、并发 SHA256、Xray→Mihomo 联测及协议版本不变。

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

- [xp-proxy 性能测试与结果治理合同](./contracts/performance.md)。

- `crates/xp-proxy-core/src/control.rs`：当前控制协议和 generation 状态机实现。
- `crates/xp-proxy/src/main.rs`：当前 listener、fallback 和 control socket 实现。
- `src/native_proxy_supervisor.rs`：当前 XP snapshot 发布与临时子进程 supervisor。
- `src/reconcile.rs`：当前 Xray reconcile 后的 native snapshot 发布位置。
- `src/ops/upgrade/managed_runtimes.rs`：当前 xp-proxy asset 安装与 host runtime upgrade 边界。
- `src/ops/runtime_activation.rs`：systemd/OpenRC readiness 语义。
- `scripts/e2e/run-local-xray-e2e.sh`：现有 Xray/Mihomo E2E 入口。
