# xp-ops：服务日志保留与轮换（#0024）

## 状态

- Status: 待实现
- Created: 2026-01-26
- Last: 2026-01-26

## 背景 / 问题陈述

- 目标运行环境适配：Arch/Debian（systemd）与 Alpine（OpenRC）。（见 `docs/desgin/requirements.md` 与 `src/ops/platform.rs`）
- 部分组件会直接写日志文件（主要是 `xray` 的 access/error log）；若缺少轮换与保留策略，磁盘空间可能被持续占用直至告警/宕机。
- 需求约束：只解决本项目相关日志文件；**不影响**系统中其他项目的日志策略（不改 systemd journal 保留、不改系统全局 logrotate 策略）。

## 目标 / 非目标

### Goals

- 为本项目相关的**日志文件**提供可配置、可审计、可回滚的“保留与轮换（log retention / rotation）”能力，避免磁盘被占满。
- 跨平台一致：覆盖 Arch/Debian/Alpine（与仓库既有 `xp-ops` 适配范围一致）。
- 行为可控：默认启用；可通过 `xp-ops init --disable-log-rotation` 关闭，且策略参数可调整。
- 幂等：重复执行 `xp-ops init` 不产生不可控漂移；`--dry-run` 可完整预览动作。

### Non-goals

- 不做全局日志策略（例如全局 journald vacuum、全局 logrotate 策略改写）。
- 不做日志集中化/采集链路（ELK/Loki/Vector 等）与告警体系设计。
- 不强制改变既有运维的日志输出方式（是否把 stdout/stderr 重定向到文件，若要做必须以显式开关控制）。
- 不提供 Web UI 调整轮换策略（仅通过 `xp-ops init` 参数调整）。

## 范围（Scope）

### In scope

- `xp-ops init` 默认启用：为“本项目日志文件”安装/更新 logrotate 配置，并安装/启用本项目专用 runner 以按计划执行轮换（执行者为系统调度：systemd timer / Alpine periodic）。
- 轮换策略默认：`size + time` 收敛（`100MiB` 与 `7 天`），执行频率默认 `daily`；策略允许通过 `xp-ops init` 参数调整。
- 仅作用于本项目日志文件：配置必须精确指向本项目相关日志文件路径（默认只覆盖 `xray` 日志）。
- 默认日志路径推导（避免要求运维手工提供路径）：
  - 优先读取 `/etc/xray/config.json` 的 `log.access` / `log.error`（若存在且为有效路径）。
  - 否则使用默认：`/var/log/xray/access.log` 与 `/var/log/xray/error.log`。
- 兼容 systemd（Arch/Debian）与 OpenRC（Alpine）。
- 文档与测试：补齐运维文档与单元测试，覆盖幂等与 `--dry-run`。

### Out of scope

- 通过修改系统级 journald 配置来限制日志体积（会影响其他项目）。
- 通过重启服务来完成日志 reopen（默认不做）。
- 让 `xp`（普通用户）直接执行文件轮换（默认不做；权限与可靠性不可控）。

## 需求（Requirements）

### MUST

- `xp-ops init` 默认启用本功能，并提供关闭开关（不需要运维额外操作就能避免磁盘被占满）。
- `xp-ops init --disable-log-rotation` 的语义为“卸载/移除（uninstall）”：必须禁用并移除本计划写入的 logrotate 配置与 runner（若存在则删除；若不存在则 no-op），且不得影响其他项目的 logrotate/journald。
- 默认策略：`size + time`，`100MiB` 与 `7 天`，执行周期默认 `daily`；允许通过参数调整阈值与周期。
- 仅影响本项目日志：不得对系统其他日志产生副作用（例如不得 vacuum 全局 journal、不得覆盖全局 logrotate status）。
- 幂等：重复执行不会不断追加/漂移；变更前后差异可控。
- 强制依赖（仅在启用轮换时）：若启用了日志轮换（默认启用），且系统缺少 `logrotate`，则 `xp-ops init` 必须失败并输出可行动的安装指引（按发行版）；当 `--disable-log-rotation` 时不得要求 `logrotate` 存在（允许在缺依赖机器上卸载 artifacts）。
- 失败可诊断：目标路径不可写等情况，返回清晰可行动错误信息（含修复建议）。
- 回滚清晰：提供关闭/卸载路径（删除本计划落盘的配置/定时器，并停止/禁用相关任务）。
- 默认不压缩轮换文件（`compress` 不启用）。
- 轮换采用 `copytruncate`（避免依赖服务的 reopen 机制）。

### SHOULD

- None

### COULD

- None

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xp-ops init`：启用日志轮换参数 | CLI | external | Modify | ./contracts/cli.md | xp-ops | operator / automation | 覆盖 Arch/Debian/Alpine |
| logrotate 配置（仅本项目日志） | File format | external | New | ./contracts/file-formats.md | xp-ops | logrotate | 仅匹配本项目日志路径 |
| 本项目专用 logrotate runner | File format | external | New | ./contracts/file-formats.md | xp-ops | systemd/OpenRC | 默认安装并启用；避免依赖系统全局调度 |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/cli.md](./contracts/cli.md)
- [contracts/file-formats.md](./contracts/file-formats.md)

## 验收标准（Acceptance Criteria）

- Given 目标系统为 Arch/Debian/Alpine，且已安装 `logrotate`，
  When 以 root 执行 `xp-ops init`（默认启用项目日志轮换），
  Then `/etc/logrotate.d/xp-ops` 被写入，且只包含本项目日志文件路径（默认仅 `xray` 的 access/error log；不包含全局通配）。
- Given 存在一个可增长的 `xray` 日志文件，
  When 触发一次本项目 runner（systemd timer / Alpine periodic），或手工运行 `logrotate`（仅作用于 `/etc/logrotate.d/xp-ops`），
  Then 当 `logrotate` 执行时，若文件大小超过阈值则触发轮换（阈值默认 `100MiB`），并确保历史保留不超过 `7 天`，且轮换文件不压缩。
- Given 已启用日志轮换且 artifacts 已存在，
  When 执行 `xp-ops init --disable-log-rotation`，
  Then 必须确保“本计划写入的工件（artifacts）”均被卸载：
  - `/etc/logrotate.d/xp-ops` 不存在
  - systemd：`xp-ops-logrotate.timer` 被 disable/stop，且相关 unit 文件被移除
  - Alpine：`/etc/periodic/<schedule>/xp-ops-logrotate` 不存在
  - `/var/lib/xp-ops/logrotate.status` 被移除（目录可保留）
  且不得触发任何 logrotate 动作。
- Given 系统不存在 `logrotate`，
  When 运维人员运行 `xp-ops init`（默认启用），
  Then 命令失败且输出必须包含对应发行版的安装建议。
- Given “不影响其他项目”的约束，
  When 启用并运行轮换策略，
  Then 不会 vacuum 系统 journal、不会改写系统级 logrotate 策略，且（若使用 runner）runner 必须使用独立的 logrotate status 文件。

## 实现前置条件（Definition of Ready / Preconditions）

- 已冻结“默认轮换的日志路径”与覆盖范围（至少包含 `xray` access/error log；其他组件是否纳入由后续计划或明确决策）。
- 已冻结 runner 形态与路径（systemd timer/service；Alpine periodic）。
- 已冻结策略参数形状（max size / max age / schedule）与默认值。
- 已冻结轮换实现细节：使用 `copytruncate`，且默认不启用 `compress`。
- CLI 契约与落盘文件契约已定稿（见 `./contracts/*.md`），实现与测试可以直接按契约落地。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: 覆盖参数解析、logrotate 配置内容、幂等写入、`--dry-run` 分支与错误信息口径。
- Integration tests: 复用 `--root <tmp>` 验证落盘路径与“不会触碰真实系统”（测试根路径下应 short-circuit 系统操作）。

### Quality checks

- `cargo fmt`
- `cargo clippy -- -D warnings`
- `cargo test`

## 文档更新（Docs to Update）

- `docs/ops/README.md`: 增加“日志保留与轮换（logrotate）”章节（启用方式、默认策略、回滚方式、常见排障）。

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones）

- [ ] M1: `xp-ops init` 生成 `/etc/logrotate.d/xp-ops`（含 xray 路径推导、copytruncate、100MiB/7d 可配）
- [ ] M2: 安装并启用 runner（systemd timer/service；Alpine periodic），使用独立 status
- [ ] M3: 测试与文档更新（`docs/ops/README.md` + 单测覆盖 `--root`/`--dry-run`/缺依赖报错）

## 方案概述（Approach, high-level）

- 以 logrotate 作为跨发行版的统一机制：为本项目日志文件生成专用配置文件，默认策略 `daily + (size/time)` 收敛到 `100MiB` / `7 天`。
- “不影响其他项目”通过两点保证：
  - 配置文件只匹配本项目日志文件路径（或由 `xp-ops` 明确生成的默认路径）。
  - runner 仅运行本项目的配置文件，且使用独立的 status 文件（不复用 `/var/lib/logrotate/status`）。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：不同服务对“日志 reopen”支持不同；若不重启服务，可能需要 `copytruncate`（存在极小日志丢失/行切割风险）。
- 风险：若依赖系统自带的 logrotate 调度（cron/systemd timer），在部分环境可能被关闭；需要决定是否由 `xp-ops` 安装本项目专用 runner。
- 假设：现网主要风险来源是 `xray` 等组件的文件日志（非 journald）。

## 变更记录（Change log）

- 2026-01-26: 创建计划。
- 2026-01-26: 调整范围为“文件日志轮换（logrotate）”，并冻结默认策略（100MiB / 7 天 / daily）。

## 参考（References）

- `docs/desgin/requirements.md`: 目标运行环境范围（Arch/Debian/Alpine；systemd/OpenRC）
- `src/ops/platform.rs`: `xp-ops` 发行版与 init system 识别逻辑
- Project X docs: `LoggerService` 可用于配合 logrotate（当前计划默认采用 `copytruncate`，不引入额外依赖）
