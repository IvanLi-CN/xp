# xp-ops：自我升级 + xp 升级 + Release 自动发布（#0018）

## 状态

- Status: 已完成
- Created: 2026-01-19
- Last: 2026-01-20

## 1) 问题陈述

当前仓库已提供运维 CLI（`xp-ops`）用于部署初始化与依赖安装（如 `xray`），但在“升级链路”上仍存在缺口：

- `xp-ops` **无法自我升级**：无法从固定来源拉取新版本并原子替换自身。
- `xp-ops` **无法对 `xp` 做一键升级**：当前 `xp-ops xp install` 仅支持从本地 `--xp-bin` 复制，缺少“从 GitHub Releases 解析版本 → 下载 → 校验 → 安装 → 重启/回滚”的闭环。
- 仓库缺少面向 `xp`/`xp-ops` 的 **Release 自动发布工作流**：即便 main 分支更新，也不会自动产出可供 `xp-ops` 拉取的 release artifacts。

本计划用于冻结“升级/发布”的口径（CLI 形状、release 产物命名、校验与回滚策略、自动发布触发方式），使 `xp-ops` 能以 GitHub Releases 作为默认分发渠道完成升级闭环。

## 2) 目标 / 非目标

### Goals

- 为 `xp-ops` 增加可脚本化的升级能力：
  - `xp-ops self-upgrade`：升级 `xp-ops` 自身。
  - `xp-ops xp upgrade`：升级 `xp`（下载 + 安装 + 服务重启 + 失败回滚）。
- 冻结 GitHub Release 的产物形状（tag/asset/checksum），确保 `xp-ops` 可稳定解析与校验。
- 增加 main 分支的自动发布版本工作流（触发策略由本计划冻结），产出 `xp`/`xp-ops` 的 release assets（与校验文件），并与现有 CI 质量门槛一致。
- 保持可测试性：升级流程需支持在 CI 中对“GitHub API / 下载 / 校验”进行 mock（不依赖真实网络状态）。

### Non-goals

- 不在本计划内引入发行版包管理器发布（Apt/Pacman/Apk/Homebrew 等）。
- 不在本计划内支持 Windows 自升级/部署。
- 不在本计划内引入复杂的灰度发布、分批升级、自动定时升级（若需要另开计划）。

## 3) 用户与场景

- **用户**：控制面管理员 / 运营（具备 root 或等价 sudo 权限）。
- **场景**
  - 在服务器上运行 `xp-ops`，无需手动拷贝新二进制，即可升级 `xp-ops` 与 `xp`。
  - main 分支合并后自动产出 release，运维只需指定 `latest` 或某个版本号即可升级/回滚。
  - 在 CI 中可对升级流程进行 dry-run 与 mock 校验，避免“线上才发现 release 产物缺失/命名不一致”。

## 4) 需求列表（MUST）

### MUST（xp-ops）

- `xp-ops` 必须提供自升级命令（见 `contracts/cli.md`）：
  - 默认从固定仓库的 GitHub Releases 拉取（默认 repo 见 `contracts/config.md`）。
  - 必须支持 `latest` 与指定版本（`vX.Y.Z` / `X.Y.Z`）解析。
  - `latest` 默认仅跟随 stable；允许通过参数选择最新 prerelease（见 `contracts/cli.md`）。
  - 必须对下载结果做完整性校验（至少 sha256；校验文件格式见 `contracts/file-formats.md`）。
  - 必须原子替换目标二进制，并在失败时保留可回滚的旧版本副本（策略见 `contracts/cli.md`）。
- `xp-ops` 必须提供 `xp` 升级命令（见 `contracts/cli.md`）：
  - 支持从 GitHub Releases 下载匹配平台的 `xp` 产物并安装到规范路径（与现有 `xp-ops init`/service files 兼容）。
  - 必须在升级后重启服务使新版本生效；重启失败必须回滚到旧二进制并返回非 0。
  - 必须支持 `--dry-run`（不下载、不写盘、不停服务，仅输出将要执行的动作与解析结果）。

### MUST（Release 自动发布）

- 必须产出可被 `xp-ops` 稳定消费的 release：
  - tag 命名规则固定（见 `contracts/file-formats.md`）。
  - assets 命名规则固定（见 `contracts/file-formats.md`），包含 `xp` 与 `xp-ops`。
  - 必须同时发布校验文件（sha256）并与 assets 一致。
- 版本号必须在每次 main 合并后自动递增：
  - 规则：`patch + 1`（`vX.Y.Z` → `vX.Y.(Z+1)`）。
  - 递增版本必须写入到构建产物里（`xp --version` / `xp-ops --version` 与 release tag 一致），且以 **环境变量注入优先** 的方式实现（见 `contracts/github-automation.md`）。
- 自动发布工作流必须包含构建前置步骤：
  - 必须先构建 `web/dist`（因为 `build.rs` 会内嵌 web assets）。
  - 必须在发布前跑过与 `ci.yml` 同级别的质量门槛（至少 fmt/clippy/test + web build）。

## 5) 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc）         | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）                       |
| --------------------------- | ------------ | ------------- | -------------- | -------------------------------- | --------------- | ------------------- | ----------------------------------- |
| `xp-ops self-upgrade`       | CLI          | internal      | New            | ./contracts/cli.md               | ops             | 管理员 / CI         | 自我升级（下载/校验/原子替换/回滚） |
| `xp-ops xp upgrade`         | CLI          | internal      | New            | ./contracts/cli.md               | ops             | 管理员 / CI         | `xp` 升级（下载/安装/重启/回滚）    |
| Release assets layout       | File format  | external      | New            | ./contracts/file-formats.md      | ci              | `xp-ops`            | tag + assets + checksums 命名与格式 |
| Upgrade config & env        | Config       | internal      | New            | ./contracts/config.md            | ops             | `xp-ops`            | repo/api-base 等输入口径            |
| Release automation workflow | Config       | internal      | New            | ./contracts/github-automation.md | ci              | ops                 | version 计算 + env 注入 + 串行发布  |

### 契约文档

- [contracts/cli.md](./contracts/cli.md)
- [contracts/file-formats.md](./contracts/file-formats.md)
- [contracts/config.md](./contracts/config.md)
- [contracts/github-automation.md](./contracts/github-automation.md)

## 6) 约束与风险

- `build.rs` 强依赖 `web/dist/index.html`：发布工作流必须先 `cd web && bun install && bun run build`，否则 Rust 构建会失败。
- 自升级属于高权限写盘操作：需要严格的原子替换与回滚策略，避免“升级失败导致工具不可用”。
- GitHub API/下载存在速率限制与网络波动：需要明确失败语义与重试策略（至少在文案与错误码上可诊断）。
- 可靠性要求高：`xp` 升级必须包含“重启 + 最小健康检查 + 失败回滚”的闭环（至少可恢复到旧版本可用状态）。
- 自动发布会与 main 的合并频率强耦合：需要在 workflow 层做并发串行化，避免版本号冲突与重复发布。

## 7) 验收标准（Acceptance Criteria）

### `xp-ops self-upgrade`

- Given 当前机器平台受支持（见 `contracts/cli.md`），且目标 release 存在匹配的 `xp-ops` asset 与 `checksums`，
  When 以 root 运行 `xp-ops self-upgrade --version latest`，
  Then `xp-ops` 下载并校验通过，原子替换自身，且再次运行 `xp-ops --version` 显示新版本；退出码为 `0`。

- Given 目标版本不存在或缺少匹配 asset，
  When 运行 `xp-ops self-upgrade --version <ver>`，
  Then 命令失败，退出码非 `0`，错误信息明确指出 “release not found / asset missing”。

- Given 校验文件存在但 checksum 不匹配，
  When 运行 `xp-ops self-upgrade ...`，
  Then 命令失败，退出码非 `0`，且不得覆盖原二进制。

### `xp-ops xp upgrade`

- Given 目标 release 存在匹配的 `xp` asset 与 `checksums`，且系统存在可管理的服务（systemd/OpenRC 其一），
  When 以 root 运行 `xp-ops xp upgrade --version latest`，
  Then `xp` 二进制被原子替换、服务被重启并成功运行；退出码为 `0`。

- Given `xp` 服务重启失败（或启动健康检查失败），
  When 运行 `xp-ops xp upgrade ...`，
  Then 必须回滚到旧二进制并尝试恢复旧服务状态（至少可再次启动旧版本），退出码非 `0`，错误信息可诊断。

- Given 以 `--dry-run` 运行，
  When 运行 `xp-ops xp upgrade --dry-run ...`，
  Then 不发生下载/写盘/停服务，仅输出解析到的 release/tag/asset 与将要执行的动作清单；退出码为 `0`。

### prerelease 选择

- Given 仓库存在 prerelease 且包含匹配平台的 assets 与 `checksums`，
  When 运行 `xp-ops self-upgrade --version latest --prerelease`（或等价参数），
  Then 选择最新 prerelease 并按相同校验/安装/回滚策略执行。

### Release 自动发布

- Given main 分支按本计划冻结的触发策略推进版本，
  When 触发发布工作流，
  Then GitHub Release 被创建（或更新），包含 `xp` 与 `xp-ops` 的所有约定 assets 与校验文件，且 `xp-ops ... upgrade` 能够稳定解析并通过校验。

## 8) 测试与质量门槛（Non-functional）

- Rust：
  - 新增/调整单元测试覆盖“release 解析、asset 选择、checksum 校验、失败分支”（推荐用 `wiremock` 模拟 GitHub API/下载）。
  - 维持仓库既有门槛：`cargo fmt --check`、`cargo clippy -- -D warnings`、`cargo test`。
- Web：
  - 维持既有构建门槛：`cd web && bun install --frozen-lockfile && bun run build`（发布工作流与 CI 一致）。

## 9) 文档更新

- `docs/ops/README.md`：补充“使用 `xp-ops` 升级/回滚”的推荐流程与注意事项（以 `contracts/cli.md` 为准）。
- （如需要）`docs/ops/cloudflare-tunnel.md`：若文档中包含安装/升级指引，保持与新命令一致。

## 10) 里程碑（Milestones）

- [x] M1: Release 自动发布工作流落地（产出 `xp`/`xp-ops` assets + checksums）
- [x] M2: `xp-ops self-upgrade` 落地（下载/校验/替换/回滚 + 测试）
- [x] M3: `xp-ops xp upgrade` 落地（下载/校验/安装/重启/回滚 + 测试）
- [x] M4: 文档补齐（ops guide 更新）与 CI 覆盖回归

## Change log

- 2026-01-20: Landed release automation + upgrade commands + tests/docs sync.
- 2026-01-20: Fixed cross multi-target build (per-target `CARGO_TARGET_DIR` to avoid glibc mismatch).

## 11) 方案概述（Approach, high-level）

- 以 GitHub Releases 为默认分发源：`xp-ops` 通过 GitHub API 拉取 release 元数据，按平台选择 asset，并以 `checksums` 做校验。
- 升级写盘采用“下载到临时文件 → 校验 → 原子替换”的流程，并保留旧版本副本用于失败回滚。
- Release 自动发布将“构建 web → 构建 Rust → 生成 checksums → 创建 release & 上传 assets”固化为一条可重复执行的流水线。

## 12) 风险与开放问题（Risks & Open Questions）

风险：

- Release 产物平台覆盖不足会导致部分发行版（例如 Alpine）无法直接使用；需要提前冻结目标平台矩阵。
- 若采用“main 每次合并自动发布”，需要明确版本号生成策略与冲突处理，避免重复发布/版本倒退。

开放问题：

None（关键口径已冻结）

## 13) 假设（需主人确认）

- 已确认：Release 触发为 **main 每次合并自动 bump + release**。
- 已确认：Release 只覆盖 Linux（`x86_64`、`aarch64`）。
- 已确认：`latest` 默认跟随 stable；通过参数允许选择 prerelease。
- 已确认：`xp-ops xp upgrade` 默认重启服务；失败回滚到旧二进制。
- 已确认：Releases 为公开仓库（不考虑私有仓库认证）。
- 已确认：版本号递增规则为 `patch + 1`。

## 参考（References）

- `xp-ops` CLI：`src/ops/cli.rs`
- GitHub release 解析（现有上游示例：xray）：`src/ops/install.rs`
- `xp` 安装/启用逻辑（现状）：`src/ops/xp.rs`
- CI 质量门槛与 web build：`.github/workflows/ci.yml`
