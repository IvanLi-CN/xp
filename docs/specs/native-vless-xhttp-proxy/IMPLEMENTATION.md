# Rust 原生 VLESS + REALITY + XHTTP 实施设计

## 当前状态

本文记录当前实现与剩余验收边界。运行时互斥、control protocol v2、持久 snapshot、
listener 生命周期、systemd/OpenRC service asset 和本地切换命令已经进入实现；在
Agent VM 的单 VM 集群、互操作、升级回滚和资源安全证据完成前，仍不广告生产 native
capability，不连接、部署或切换 us3。

需求合同见 [SPEC.md](./SPEC.md)；兼容性基线见 [COMPATIBILITY.md](./COMPATIBILITY.md)。

## 性能需求与实现状态

性能合同见 [contracts/performance.md](./contracts/performance.md)，设计预算见
[PERFORMANCE-DESIGN.md](./PERFORMANCE-DESIGN.md)。当前实现候选已增加独立构建、测量、门禁与人工标记工作流，
以及默认并发、首包解析、流许可释放、出站连接池、H2 流控和 TLS 分片读取修复。TLS record 后端使用缓存密钥的 ring AES-GCM，
两种套件均与独立 AES-GCM 实现进行字节、序号和认证失败对照；不改变 record 格式或 release 优化级别。
确定性回归已覆盖默认 64 并发、大首包、流许可、连接池和取消分片。
TLS 应用数据通过有界的 `AsyncRead/AsyncWrite` record stream 直接交给 H2，移除后台桥接任务和 plaintext duplex。

旧桥接在接收积压时阻塞反向发送；确定性回归在直接 I/O 修复后通过。读写分别保留分片和待发密文偏移，取消后不重复确认 plaintext 或重加密。

待发密文以单条 record 为界。关闭先发出已确认数据和 `close_notify`，写半关闭后仍允许读取响应。认证失败、超大或截断 record 均拒绝。

默认 Tokio worker 数和运行内存配置不变。本地功能回归不替代当前 SHA 的性能验收。
GitHub-hosted 同场矩阵已暴露并保留 main 的既有并发/流控失败；候选与 Xray 的完整矩阵正在按最终 SHA 收敛。
新增多目标/多客户端隔离 fixture，本地功能诊断已覆盖 8 目标、16/64 流和慢/失败目标混合；
Linux 验证实际底层连接数及当前 SHA 的 Actions 证据；CPU/PSS 摘要使用每个负载区间的
差分与采样峰值。内存上限等排障实验仅保留临时证据，不交付实验脚本或工作流。
原始测量不得因主干缺陷被自动改为成功；人工 `perf:waive` 只改变 PR 门禁。

当前尚无本候选的 GitHub-hosted 轻量/全量验收结论，也未完成正式审查；不得视为性能达标或 merge-ready。
人工作废工作流须合并到默认分支后才有真实入口，pre-merge 负例不能代替 post-merge 实测。
v3.48.15 的探索性测试暴露默认并发失败及延迟/吞吐差距；本地原始证据位于忽略目录 `.run/xp-proxy-perf-20261117/`，不是持久提交的验收证据。

其单 VM 短测不能代替正式同场主干/Xray 对照、持续稳定性或生产承载证明。

| 阶段                       | 轻量工程预算 | 全量工程预算 |
| -------------------------- | -----------: | -----------: |
| checkout、下载、校验与准备 |        30 秒 |        30 秒 |
| 直连基线与环境记录         |         6 秒 |         6 秒 |
| 启动、就绪与预热           |         6 秒 |         6 秒 |
| 三程序同场采样             |        72 秒 |       288 秒 |
| 默认功能、收尾与资源检查   |         6 秒 |         6 秒 |
| 报告与证据上传             |        12 秒 |        12 秒 |
| 余量                       |        48 秒 |        12 秒 |
| 总设计预算                 |       180 秒 |       360 秒 |

采样设计为两次顺序相反的 sweep：轻量 `1 方向 × 2 并发档 × 3 负载 × 3 程序 × 2 sweep × 2 秒`，全量增加为 `2 方向 × 4 并发档`。

每个两秒槽包含有界停止/收尾；不将无界排空叠加在预算外。此分配尚未实测，尤其全量余量较小；如果无法同时提供可靠样本与目标墙钟，必须报告设计未验证，不削减矩阵或放宽门槛。

性能门槛、预算、门禁状态和人工标记需要各自的确定性负例及实际 Actions 证据。文档检查不代表这些运行门禁已经通过；既有协议、host 生命周期与部署证据保持独立。

## 现有实现与差距

- **进程生命周期：** `xp-ops` 负责独立服务的启动、停止和运行时切换，XP 不拥有
  `xp-proxy` 子进程生命周期。XP 只通过 control socket 协调，且 Xray 与 xp-proxy
  必须由节点级运行时状态互斥控制。
- **Snapshot 发布：** XP 仅在环境开关和 `/etc/xp/runtime.json` 明确为
  `xp-proxy/active` 时发送完整 snapshot；缺失、切换中或失败状态均 fail closed。
  endpoint owner 仍由既有 `ingress_owner` 语义约束，旧 metadata 默认 Xray。
- **Generation：** snapshot generation 已进入持久状态文件，并在 control protocol v2
  中按 active generation 单调检查；同一 prepared payload 可幂等重放，冲突 payload 或
  不同 generation 会被拒绝。
- **状态机：** `GenerationManager` 已持久化最近成功 snapshot，支持 prepare、activate、
  drain、stop、rollback 和结构化状态；重启只恢复诊断 snapshot，不自动恢复对外 listener。
- **Listener：** `xp-proxy` 在 activate、stop 和 rollback 前等待旧 listener task 移除并
  释放端口；部分 bind 失败会清理已启动 listener，禁止通过 `SO_REUSEPORT` 绕过冲突。
- **Fallback：** 当前路径选择第一个 fallback，并以固定目标 allowlist 进行转发；HTTP/H2
  语义和 header 清理沿用现有 canary 合同，未声明任意 URL 或任意 authority 转发。
- **重启恢复：** `xp-proxy` 已持久化 last-successful snapshot 供诊断，但重启后保持 idle，
  等待 XP 以新请求重放 generation，不服务陈旧 snapshot。
- **服务安装：** release 已包含独立 `xp-proxy` service asset；`xp-ops init` 创建
  `xp-proxy` identity、socket 目录和持久目录。XP 通过 `xp-proxy` group 访问 socket，
  init 不自动启动该服务；显式 runtime switch 才启动目标入口。
- **兼容性：** 已有协议单测和 Xray/Mihomo XHTTP fixture；native host handoff、两种 init、
  四条协议路径、fault injection 和资源门禁仍待 Agent VM 绑定候选 SHA 验证。

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

下列决策已由本任务锁定，以下记录用于审查实现边界，不再作为未决开发门禁。

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
  self-test 和 owner proof；本 PR 已加入运行时状态门禁和 listener/generation 基础合同，
  完整 capability 与真实流量证据仍是发布门禁。
- **Xray/native owner：** 需要 desired config、native snapshot、端口采样和 operation
  journal 一致；本 PR 已禁止非 `xp-proxy/active` 节点发布 snapshot，endpoint owner 与
  Xray desired config 的完整 handoff 仍需集成证据确认。
- **Control protocol：** 已升级为 versioned/idempotent request、status、listener release
  和 rollback；unknown outcome 与跨服务 operation journal 仍由集成测试覆盖。
- **Persistence：** XP 的 generation 与 proxy last-successful snapshot 已持久化；重启不
  自动恢复 listener，必须由 XP 重放新 snapshot。
- **Fallback：** 需要四种 scheme 的协议级 fixture、authority/header 安全和上游
  故障。
  当前未满足，主要是首个目标 raw relay。
- **systemd：** 已有独立 service、专用 identity、受限 socket 目录和显式 switch；ready、
  drain、upgrade、rollback 仍需 Agent VM 生命周期证据。
- **OpenRC：** 已有独立 init script、专用 identity 和 supervisor；两次连续 ready/stopped、
  upgrade、rollback 仍需 Agent VM 生命周期证据。
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
