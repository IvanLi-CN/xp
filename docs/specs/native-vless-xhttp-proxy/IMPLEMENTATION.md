# Rust 原生 VLESS + REALITY + XHTTP 实施设计

## 当前状态

本文记录需求与设计锁定结果，不代表功能已实现或可部署。
当前版本不广告 native production capability，不连接、部署或切换 us3，
也不改变任何生产 endpoint owner。

需求合同见 [SPEC.md](./SPEC.md)；兼容性基线见 [COMPATIBILITY.md](./COMPATIBILITY.md)。

## 现有实现与差距

- **进程生命周期：** `src/native_proxy_supervisor.rs` 由 XP 拉起 `xp-proxy` 子进程，
  `run_server` 与 Xray 同时启动。实现前需改为 systemd/OpenRC 独立 service，
  XP 只通过 control socket 协调。
- **Snapshot 发布：** XP 在 Xray reconcile 成功后发送 `Prepare`/`Activate`，
  但没有 owner 排他和 activate 结果门禁。实现前需增加显式 `ingress_owner`，
  将 native endpoint 从 Xray desired config 排除，并使用协调 operation。
- **Generation：** `NEXT_GENERATION` 是进程内 `AtomicU64`，XP 重启后从 1 开始。
  实现前需由 XP 持久化按节点单调递增的 generation，并绑定 snapshot digest。
- **状态机：** `GenerationManager` 只有内存 active/prepared，没有持久化、显式
  rollback 或真实 listener drain。实现前需加入 durable state、幂等 request、
  结构化错误、
  rollback intent 和 listener handle 生命周期。
- **Listener：** `xp-proxy` 绑定后只保存在 `ListenerRuntime`，`Drain`/`Stop` 不会
  释放旧 listener，同 bind 还可能被跳过。实现前需记录 generation owner，完整
  stop/drain，
  并在部分 bind 失败时原子清理。
- **Fallback：** 当前路径选择第一个 fallback，并以 raw TCP relay 为主；HTTP/H2
  语义和 header 清理没有形成 native 合同。实现前需在主人确认 scheme
  语义后实现 allowlist、
  authority、协议分流和 hop-by-hop 清理。
- **重启恢复：** `xp-proxy` 没有生产级 last-successful snapshot persistence，XP
  也不确认 active 状态。实现前需持久化校验/诊断信息，重启后等待 XP 重放
  generation，
  不得服务陈旧 snapshot。
- **服务安装：** release 会安装 `xp-proxy` binary，但 init、service readiness、upgrade
  activation 只覆盖 XP/Xray/cloudflared。实现前需为 systemd/OpenRC 增加独立 asset、
  权限、
  ready/stopped 两次观测、升级与回滚顺序。
- **兼容性：** 已有协议单测和 Xray/Mihomo XHTTP fixture，但没有 native host handoff/
  upgrade 证据。实现前需在 Agent VM 完成两种 init、四条协议路径、fault injection
  和资源门禁。

## 已锁定的设计

以下内容来自用户任务、现有有效 SPEC/ADR 和仓库 deployment contract，
本轮作为实现前不可再隐式改变的边界：

- XP 是控制面唯一权威，`xp-proxy` 是独立 Rust 数据面；数据面不读取 Raft、
  SQLite 或 Web。
- endpoint 的 `transport=xhttp` 不等于 native；必须有独立的 ingress owner，旧 metadata
  缺省为 Xray。
- 同一端口只能有一个 owner。Xray 与 native 不得同时 bind，也不得用 bind race 或
  `SO_REUSEPORT` 伪造无中断切换。
- `xp-proxy` 的二进制、进程、control socket、prepare 或 `ready` 单独都不算 usable
  ingress；必须有 active generation、全部 listener、self-test、owner 排他和兼容性
  证据。
- native 不可用时不自动把端口交给 Xray；服务管理器可以重启相同的
  `xp-proxy`。跨数据面 rollback 只能由显式协调操作完成。
- generation 必须由 XP 持久化，陈旧本地 snapshot 不能在重启后直接对外服务。
- host-managed systemd/OpenRC 是首个实现目标；Docker/Compose 在具备等价合同前保持
  native capability 缺失，继续使用当前 Xray data plane。
- 首个协议基线固定为 Xray `26.3.27`、Mihomo `v1.19.29`、TCP VLESS、REALITY、XHTTP
  `stream-one`、六项 XMUX，以及明确的 HTTP/HTTPS/H2/h2c fallback。

## 设计前沿：必须由主人确认

下列问题不能从现有代码或稳定合同推断。以下给出推荐值和理由，
但在主人确认前不得把推荐值当作实现授权；它们是 Stage 1 的 hard gate。

1. **Service identity：** 推荐使用专用 `xp-proxy` OS user/group，XP 仅通过受限 group
   访问 control socket，私钥与 snapshot 只允许 proxy 读取。
   需要确认是否接受新增 service identity 和端口绑定权限，或明确接受复用
   `xp` user。若接受复用，需要明确接受更宽的数据面权限。
2. **Generation 来源：** 推荐由 XP 为每个节点维护持久的 monotonic proxy revision，
   发布前保留 revision，并让 snapshot digest 与 revision 一起校验。
   需要确认是否采用按节点 revision，或改用能完整代表本机 snapshot 且支持
   rollback ordering 的 Raft commit index。
3. **Capability / metadata 名称：** 推荐 node-local capability
   `runtime.native-vless-xhttp-v1`，endpoint metadata 使用 `ingress_owner`，取值为
   `xray` 或 `native`，缺失按 `xray` 处理。
   需要确认 capability 字符串、字段名、API 读写入口，
   以及 capability 是否为节点本地、是否要求全体 voter 宣告。
4. **单端口中断预算：** 推荐接受有界 handoff 中断，先 drain/stop 旧 owner，
   确认端口释放，再 bind 新 owner，不承诺 zero-downtime。
   需要确认最大中断、drain deadline，以及是否另行批准 socket activation/FD
   handoff 设计。
5. **Drain 超时：** 推荐 120 秒硬 deadline，到期关闭剩余 session 并进入
   `failed`/`rollback_required`。
   需要确认具体秒数、是否 hard close，以及升级时是否允许延长。
   不得让 drain 无限等待。
6. **Fallback 语义：** 推荐沿现有 canary：HTTP/H2/h2c 做协议级 request relay 并清理
   hop-by-hop headers，HTTPS 做固定 authority 的 TLS tunnel，所有目标仍是 exact
   allowlist。
   需要确认四种语义、redirect、TLS 校验、DNS resolution 和 loopback target 规则。
7. **Native 崩溃恢复：** 推荐只重启 native 并由 XP 重放当前 snapshot，
   不自动切回 Xray；人工/显式 API 才能做 owner rollback。
   需要确认是否允许受条件约束的自动 rollback。
   若允许，必须给出触发阈值、审计和双 owner 防护。
8. **Docker 首发范围：** 推荐 host-managed 首先落地，Docker 继续 Xray，直到有
   等价 service/upgrade evidence。
   需要确认 native 是否必须与同一 release 的 Docker 一起交付，若必须，需增加
   容器内 supervision 与 owner handoff 设计。

未得到这些确认前，不能通过“已实现”“可用于生产”或
“可切换流量”的验收表述。

## 推荐的切换状态机

### Xray -> native

1. XP 线性一致读取 endpoint owner、node capability、端口占用、当前 generation 和
   pending operation。
2. 建立带 operation id 的切换记录，snapshot 只允许包含被选为 native 的 endpoint；
   其余 endpoint 继续由 Xray 管理。
3. 向 `xp-proxy` prepare 完整 candidate。prepare 只做 schema、crypto、fallback、资源和
   loopback self-test，不宣称公开 ingress。
4. 更新 Xray desired config，删除同一 endpoint inbound；等待 Xray 报告 ready 且通过
   端口空闲检查。
5. activate native candidate；确认全部 listener、active generation、握手和 fallback
   self-test。
6. 只有第 5 步成功才提交 endpoint owner=native；否则按失败原因恢复 Xray。
   恢复失败则 endpoint 保持 stopped/failed，不启动两个 owner。

### native -> Xray

1. 记录显式 rollback/switch operation 和 expected native generation。
2. 让 native 停止新 session，进入 bounded drain；到 deadline 后按已确认策略关闭
   剩余 session，释放所有 listener。
3. 验证端口为空并恢复 Xray inbound；等待 Xray ready、握手 self-test 和 endpoint
   owner 一致。
4. 成功后提交 owner=xray；任一步失败则保持 stopped/failed，
   不让 XP 自动再 bind native。

这两个流程没有隐含 zero-downtime 承诺；未知 control response 必须先 status
查询，不能直接发送相反 owner 的 mutation。

## Host-managed 安装、升级与回滚顺序

### 首次安装

1. `xp-ops init` 创建独立 service asset、受限 socket 目录、数据目录和明确的
   service identity；不因为安装 binary 就广告 capability。
2. systemd 启动 `xp-proxy.service`，OpenRC 启动 `xp-proxy`；两者都必须报告 socket、
   协议 status 和 process readiness。OpenRC 必须两次连续确认 ready。
3. XP 启动后只发布当前完整 snapshot；proxy 在确认 generation 前不接收陈旧本地
   snapshot。
4. XP 通过 owner/handoff 状态确认后，System Status 才显示 `active_ingress`。

### 升级

1. 锁定 release、取得升级 advisory lock，读取当前 endpoint owner 和 active
   generation。
2. 对 native endpoint 先执行 explicit drain/stop，确认 `xp-proxy` 已停止监听。
   XP、Xray、cloudflared 和 proxy 进入同一 managed-runtime activation window。
3. 下载并校验 XP、Xray、cloudflared、xp-proxy 资产；每个目标使用现有
   transaction-local backup contract，不改变 `/var/backups/xp`、凭据、证书和 Raft
   数据。
4. 替换并 reload service assets；先确认 Xray/cloudflared 与 `xp-proxy` ready，
   再启动 XP，由 XP 重放 snapshot 并重新确认 owner/generation。
5. 任一 runtime、snapshot、owner 或 readiness 失败，都按现有 rollback contract 恢复
   匹配二进制和 service asset。恢复的 proxy 不能 ready 时，endpoint 保持 stopped；
   不能用新 Xray 抢占 native 端口。
6. 只有服务 readiness、snapshot active、owner 一致和事务备份清理全部完成，
   才能报告升级成功。`rollback_failed` 与 `cleanup_incomplete: committed_upgrade`
   语义保持不变。

## 分阶段交付

- **Stage 0：** 需求、术语、ADR、SPEC、ops 合同已锁定，待主人确认 8 项决策。
  退出条件是主人明确确认或修订决策前沿，且无隐含假设。
- **Stage 1：** control protocol v2、持久 generation、snapshot persistence、幂等、
  rollback 和 listener lifecycle。退出条件是 `VER-SNAPSHOT`、`VER-GENERATION`、
  `VER-CONTROL`、`VER-ROLLBACK` 通过。
- **Stage 2：** endpoint owner、Xray reconcile 排他、XP handoff coordinator、status。
  退出条件是 `VER-OWNER` 通过，故障时无双 bind、无自动 failback。
- **Stage 3：** systemd/OpenRC 独立 service、权限、ready/stopped、init/upgrade/rollback。
  退出条件是 Agent VM 两种 init 的 install/upgrade/rollback evidence 完整。
- **Stage 4：** native 协议与 fixed fallback 完整实现。
  已有 spike，但生产证据未完成。
  退出条件是 Xray/Mihomo 四条路径及 fallback matrix 全通过。
- **Stage 5：** 资源、安全、故障注入、发布门禁。
  退出条件是资源、secret、symlink、crash、unknown outcome 和审计证据完整。
- **Stage 6：** 明确授权的受控 canary，不在本轮。
  需要另行 owner 授权、发布门禁和流量窗口；本轮不部署 us3。

## Agent VM、Xray、Mihomo 验证边界

### 本地 Rust 验证

- control frame 长度、版本、request id、digest、重复请求和结构化错误。
- generation 持久化、重启、低 generation、显式 rollback 和 snapshot 损坏。
- listener 全量 bind/rollback/drain、空 snapshot、部分失败清理和连接计数。
- VLESS framing、UUID auth、REALITY 正/负握手、XHTTP stream-one、flow control、取消。
- 六项 XMUX 范围、复用/过期/保活/断线重连及错误配置拒绝。
- fallback URL、scheme、authority、Host/SNI、header 清理和任意目标拒绝。

### Agent VM 必测

- Linux systemd 和 OpenRC 的 service user/group、socket mode、目录权限、symlink 防护、
  低端口绑定和两次连续 ready/stopped 观测。
- XP、Xray、cloudflared、xp-proxy 的真实 stop/drain/start 顺序；异步退出 0 但服务未
  ready 的负例；proxy crash、socket 消失、XP crash、重复发布和重启恢复。
- 同端口 Xray/native 竞争、两端口并行运行、Xray->native 和 native->Xray handoff，
  证明任何时刻只有一个 listener owner。
- release asset 安装、校验失败、服务激活失败、rollback_failed、事务 backup 保留
  和 committed cleanup；不连接生产节点。
- FD、H2 stream、连接、内存、日志和 status payload 上界，错误状态不得渲染为
  ready/zero。

### 固定 Xray/Mihomo 互操作

- Xray `26.3.27` outbound -> native inbound；native outbound -> Xray `26.3.27` inbound。
- Mihomo `v1.19.29` YAML/URI -> native inbound；native outbound -> Mihomo-compatible server。
- REALITY SNI、ALPN、fingerprint、short ID、key pair、错误凭据、时间窗口、畸形
  ClientHello。
- 小包、大包、至少 64 个并发 stream、预热后 idle reuse、主动断开、重连。
  另外覆盖半关闭、连接池上限、外层 TCP 建连数和 native session 数。
- HTTP、HTTPS、H2、h2c fallback 的成功、authority mismatch、错误 scheme、redirect、
  hop-by-hop header、上游断开和固定 canary 失败。
- native endpoint 与普通 Xray endpoint 在不同端口并行，以及 owner 切换期间的
  停止/恢复行为；同端口双 owner 负例必须 fail closed。

## Acceptance matrix

- **Usable ingress：** 需要 capability、process/socket、active generation、全部 bind、
  self-test 和 owner proof。当前未满足，只是实验性 snapshot path。
- **Xray/native owner：** 需要 desired config、native snapshot、端口采样和 operation
  journal 一致。当前未实现，reconcile 仍可能同时配置 Xray/native。
- **Control protocol：** 需要 versioned/idempotent request、status、unknown outcome 和
  rollback。当前未满足，只有内存 v1 manager。
- **Persistence：** 需要 XP durable generation 与 proxy last-successful snapshot recovery。
  当前未满足，generation 会在进程中重置。
- **Fallback：** 需要四种 scheme 的协议级 fixture、authority/header 安全和上游
  故障。
  当前未满足，主要是首个目标 raw relay。
- **systemd：** 需要独立 service、ready、drain、upgrade、rollback。
  当前没有 xp-proxy unit。
- **OpenRC：** 需要两次连续 ready/stopped、supervision、upgrade、rollback。
  当前没有 xp-proxy init script。
- **Xray/Mihomo：** 需要固定版本四条路径与资源 evidence。
  当前只有 Xray/XHTTP spike，native host lifecycle 仍未完成。
- **Docker：** 按设计保持 capability 缺失，不得误报为支持。
- **us3/production：** 无部署、无流量切换、无 release publication，
  本轮明确未执行。

## Implementation file targets

实现阶段应优先修改并配套测试以下边界：

- `crates/xp-proxy-core/src/control.rs`：生产 control protocol、状态、generation、
  rollback、persistence 和结构化错误。
- `crates/xp-proxy/src/main.rs`、`xhttp_h2.rs`、fallback 模块：真实 listener lifecycle、
  stream semantics 和 scheme-aware fallback。
- `src/native_proxy_supervisor.rs`、`src/reconcile.rs`：去除 XP 子进程 supervisor，
  加入 endpoint owner、XP handoff 和 active status 观察。
- `src/ops/init.rs`、`src/ops/runtime_activation.rs`、`src/ops/paths.rs`：systemd/OpenRC
  service、权限、ready/stopped 和路径合同。
- `src/ops/upgrade/managed_runtimes.rs`：proxy 纳入 activation、rollback、backup 和 cleanup。
- `docs/ops/systemd/`、`docs/ops/openrc/`、容器入口与 release workflow：资产、版本和
  capability 边界同步。

本清单只是实现边界，不授权在本轮直接实施或部署；
所有阶段都必须先满足主人确认，实现还必须满足 SPEC acceptance criteria。
