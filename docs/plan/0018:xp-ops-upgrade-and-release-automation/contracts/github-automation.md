# GitHub automation Contracts（#0018）

本文件用于冻结“PR labels 与 Release 自动发布”的仓库级约定，确保版本号递增、发布产物与 `xp-ops` 升级链路一致且可预期。

## Versioning policy

- 版本号为 semver（`X.Y.Z`）。
- 自动递增规则（normative）：每次 main 合并后 `patch + 1`（`vX.Y.Z` → `vX.Y.(Z+1)`）。
- Release tag 格式与 assets 命名见 `contracts/file-formats.md`。

## Source of truth (build-time injection)

为保证 `xp --version` / `xp-ops --version` 与 release tag 一致，版本号必须在 CI 构建时优先从环境变量注入（normative）。

- `XP_EFFECTIVE_VERSION`：release 工作流计算出的“本次发布版本”（例如 `0.1.23`）。
- `XP_BUILD_VERSION`：传入 `cargo build` 的编译期版本变量；其值必须等于 `XP_EFFECTIVE_VERSION`。
- 本仓库不要求在 release 工作流中改写 `Cargo.toml`；`Cargo.toml` 仅用于提供初始版本与（可选）major/minor 的人工变更入口。

## Release automation workflow (main)

触发：

- `push` to `main`（每次合并触发）

行为（normative）：

- 串行化：同一时刻只允许 1 个 release 工作流运行（使用 `concurrency` 且 **不取消** in-progress，以保证每次合并都有 release）。
- 计算 `XP_EFFECTIVE_VERSION`（建议用脚本固化，参考 `tavily-hikari` 的 `.github/scripts/compute-version.sh` 形态）：
  - 读取 `Cargo.toml` `package.version`（记为 `X.Y.Z`）
  - 执行 `git fetch --tags --force`（确保 tags 可用）
  - 从 `candidate=Z` 开始，循环检查 `vX.Y.<candidate>` 是否存在：
    - 若存在则 `candidate=candidate+1` 继续
    - 直到找到一个不存在的 tag
  - 产出：`XP_EFFECTIVE_VERSION = X.Y.<candidate>`
- 构建并发布（构建阶段必须注入版本环境变量）：
  - 设定 `XP_BUILD_VERSION=$XP_EFFECTIVE_VERSION`
  - 先构建 `web/dist`（以满足 `build.rs`）
  - 构建 `xp` 与 `xp-ops` 的 Linux musl 产物（targets 见 `contracts/file-formats.md`）
  - 生成并上传 `checksums.txt`
  - 创建（或更新）GitHub Release：tag 为 `v<XP_EFFECTIVE_VERSION>`

## Repository setup

- None.
