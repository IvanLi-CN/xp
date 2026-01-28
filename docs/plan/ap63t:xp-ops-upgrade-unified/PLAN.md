# xp-ops：统一升级命令 `upgrade`（#ap63t）

## 状态

- Status: 已完成
- Created: 2026-01-28
- Last: 2026-01-28

## 背景 / 问题陈述

- 当前 `xp-ops` 的升级入口分散：`self-upgrade` 负责 `xp-ops` 自身，`xp upgrade` 负责 `xp`，用户容易困惑与误用。
- 目标使用场景是“运维只记一个升级命令”，并且一次执行就把 `xp` 与 `xp-ops` 同步到最新版本。

## 目标 / 非目标

### Goals

- 提供单一入口：实现 `xp-ops upgrade` 命令，一次同时升级 `xp` 与 `xp-ops` 到同一个 release（默认 latest stable）。
- `xp` 本体 CLI 不出现“自更新/升级”相关命令。
- 保持兼容性：既有 `xp-ops self-upgrade` 与 `xp-ops xp upgrade` 仍可运行，但从帮助输出中隐藏，并在执行时提示迁移到 `xp-ops upgrade`。

### Non-goals

- 不引入新的发布渠道（仍基于 GitHub Releases + `checksums.txt` + SHA256 校验）。
- 不新增 `xp` 二进制自身的升级逻辑。
- 不改变安装路径与回滚策略（仍为原子替换 + `.bak.<unix-ts>` 备份）。

## 范围（Scope）

### In scope

- `xp-ops upgrade`：封装并串联升级 `xp` 与 `xp-ops`（支持 `--version/--prerelease/--repo/--dry-run`）。
- `xp-ops self-upgrade` 与 `xp-ops xp upgrade`：在 CLI 帮助中隐藏；执行时输出 deprecate 提示（stderr）。
- 文档更新：统一推荐命令改为 `xp-ops upgrade`。
- 测试：增加/更新单测覆盖新命令（以及旧命令的兼容提示如适用）。

### Out of scope

- 改动 `xp` 的 CLI 命令集（除保证“不出现更新命令”外，不做其它重构）。
- 变更 `xp-ops` 的 install/init/deploy 等其它功能。

## 需求（Requirements）

### MUST

- `xp-ops upgrade` 默认同时升级 `xp` 与 `xp-ops`（最新稳定版）。
- 支持 `--dry-run`：不写入/不下载二进制资产（允许只解析 release 并打印将要执行的动作）。
- 支持 `--prerelease`（仅与 `--version latest` 组合）与 `--repo` 覆盖仓库来源（与现有升级命令语义一致）。
- 升级失败的回滚语义与原有命令保持一致：
  - `xp` 升级失败（校验失败/重启失败等）应回滚并以非 0 退出码返回。
  - `xp-ops` 自升级失败应回滚并以非 0 退出码返回。
- `xp` 二进制自身不提供升级/自更新命令。

### SHOULD

- 旧命令在执行时提示：`deprecated: use xp-ops upgrade`，并给出等价命令示例。
- 文档中不再推荐使用旧命令路径。

### COULD

- 在 `xp-ops upgrade` 输出中显示两个步骤的结果摘要（例如 “xp upgraded”, “xp-ops upgraded / already up-to-date”）。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xp-ops upgrade` | CLI | internal | New | ./contracts/cli.md | ops | 管理员 / CI | 单一入口升级 `xp` + `xp-ops` |
| `xp-ops self-upgrade` | CLI | internal | Modify | ./contracts/cli.md | ops | 管理员 / CI | 隐藏 + deprecated 提示 |
| `xp-ops xp upgrade` | CLI | internal | Modify | ./contracts/cli.md | ops | 管理员 / CI | 隐藏 + deprecated 提示 |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/cli.md](./contracts/cli.md)

## 验收标准（Acceptance Criteria）

- Given 已安装 `xp-ops` 与 `xp`（`/usr/local/bin/xp` 存在），
  When 以 root 运行 `xp-ops upgrade --version latest`，
  Then `xp` 会被下载/校验/替换并尝试重启服务，且 `xp-ops` 会自升级到同一 release（若已是目标版本则输出 up-to-date 并返回成功）。
- Given 任意环境，
  When 运行 `xp-ops upgrade --dry-run ...`，
  Then 只输出解析到的 release 与两段升级动作计划，不下载/不写入/不重启。
- Given 旧脚本仍在使用 `xp-ops self-upgrade` 或 `xp-ops xp upgrade`，
  When 执行旧命令，
  Then 命令仍可工作，但 stderr 会提示迁移到 `xp-ops upgrade`，且 `--help` 不再展示旧命令。
- Then `xp` CLI 中不存在升级/自更新命令。
- Then 文档 `README.md` 与 `docs/ops/README.md` 的升级指引统一为 `xp-ops upgrade`。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: 覆盖 `xp-ops upgrade` 的 dry-run 解析与“同时升级两类资产”的行为（mock GitHub API）。
- Integration tests: 维持现有升级相关测试（必要时调整为调用新命令）。

### Quality checks

- Rust: `cargo fmt`, `cargo test`, `cargo clippy -- -D warnings`

## 文档更新（Docs to Update）

- `README.md`: “Ops tool” 描述与升级推荐命令改为 `xp-ops upgrade`
- `docs/ops/README.md`: “Upgrade and rollback strategy” 改为统一升级命令

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones）

- [x] M1: 增加 `xp-ops upgrade` CLI + preflight + 实现串联逻辑
- [x] M2: 隐藏旧命令并添加 deprecated 提示；更新文档指引
- [x] M3: 更新/新增测试覆盖 `xp-ops upgrade`

## 方案概述（Approach, high-level）

- 保留现有升级实现（下载、校验、原子替换、回滚、服务重启），新增一个顶层命令作为编排层。
- 对旧命令保持兼容，但将其从帮助输出中隐藏，并在运行时提示迁移。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：旧命令从帮助中隐藏可能影响习惯用法；通过执行时提示与文档更新缓解。
- 假设：`xp` 与 `xp-ops` 同一 release tag 下均存在对应平台资产与 `checksums.txt`。

## 变更记录（Change log）

- 2026-01-28: 创建计划
