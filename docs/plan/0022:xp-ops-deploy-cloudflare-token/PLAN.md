# `xp-ops deploy`：支持从命令行传递 Cloudflare API token（#0022）

## 状态

- Status: 已完成
- Created: 2026-01-23
- Last: 2026-01-24

## 1) 问题陈述

当前 `xp-ops deploy` 在启用 Cloudflare 相关能力时，需要 Cloudflare API token 才能完成预检与派生字段（例如 `zone_name`、`hostname`）的推导与校验；但 token 只能来自环境变量 `CLOUDFLARE_API_TOKEN` 或本地文件 `/etc/xp-ops/cloudflare_tunnel/api_token`，导致“一条命令完成 deploy”的场景下无法把 token 作为命令行参数传入，预检会以 `token_missing` 失败。

## 2) 目标 / 非目标

### Goals

- 为 `xp-ops deploy` 增加“命令行可传递 token”的入口，以便一条命令完成 deploy 预检与执行（无需预先写入 token 文件，也无需依赖 `sudo -E` 透传环境变量）。
- token 属于敏感信息：命令输出/预检摘要/错误信息不得泄露 token 原文。
- 保持既有 token 来源兼容：不破坏当前的 `CLOUDFLARE_API_TOKEN` 与 `/etc/xp-ops/cloudflare_tunnel/api_token` 读取逻辑。

### Non-goals

- 不改变 Cloudflare API token 的权限模型与 Cloudflare 侧资源语义（Tunnel/DNS/Ingress 的业务含义不变）。
- 不引入新的依赖或新的配置中心；不改变既有文件格式（除非主人明确要求把 CLI token 默认落盘保存）。
- 不重做 `xp-ops cloudflare token set` 等既有子命令的职责划分。

## 3) 用户与场景

- 运维人员在新机器上一次性执行 deploy：希望把所有必需参数（包含 token）都通过命令行传入，预检可通过并完成派生字段推导。
- CI/自动化脚本：希望无需 `sudo -E`，仍可通过命令行显式传递 token（同时要求输出不泄露 token）。

## 4) 需求列表（MUST）

- MUST: `xp-ops deploy` 提供一个可选参数用于传入 Cloudflare API token（见 `contracts/cli.md`），并在启用 Cloudflare 时参与 preflight 与后续 Cloudflare API 调用。
- MUST: token 解析优先级必须明确且可测试：
  - 当命令行 token 参数存在时，必须优先使用它；
  - 否则保持现有逻辑：先读 `CLOUDFLARE_API_TOKEN`，再读 `/etc/xp-ops/cloudflare_tunnel/api_token`。
- MUST: 任何输出不得包含 token 原文（包括 preflight config、errors、dry-run 输出）。
- MUST: 当 Cloudflare 启用且 token 缺失时，错误信息必须可操作：明确指出可用的 token 来源（CLI 参数 / env / 文件路径）。
- MUST: 当 token 通过命令行参数提供时，成功部署后必须提示用户“该方式有泄露风险，建议立即轮换/废弃该 token”（不输出 token 原文）。
- MUST: 不改变现有非 Cloudflare 场景下 `xp-ops deploy` 的行为与参数校验。

## 5) 接口清单与契约（Interfaces & Contracts）

| 接口（Name）    | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc）               | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）               |
| --------------- | ------------ | ------------- | -------------- | -------------------------------------- | --------------- | ------------------- | --------------------------- |
| `xp-ops deploy` | CLI          | external      | Modify         | [contracts/cli.md](./contracts/cli.md) | ops             | operators / CI      | 新增 token 参数与优先级规则 |

## 6) 约束与风险

- token 作为命令行参数会进入 shell history 与进程列表（`ps`）：虽然满足“可从 CLI 传参”，但安全性更弱；需要在文档中明确推荐优先使用 env 或落盘文件。
- 为降低上述风险，本计划要求额外支持“从 stdin 读取 token”（见 `contracts/cli.md`）。
- 需要保证“token 不出现在任何日志/错误/预检摘要”在所有路径上都成立（包括失败分支）。

## 7) 验收标准（Acceptance Criteria）

- Given 以 root 运行 `xp-ops deploy` 且启用 Cloudflare，
  When 通过命令行参数传入 Cloudflare API token（`--cloudflare-token` 或 `--cloudflare-token-stdin`；见 `contracts/cli.md`），
  Then preflight 不再出现 `cloudflare token error: token_missing`，并能派生/校验 `zone_name` 与 `hostname`（在参数足够时），最终不因 token 缺失而失败。

- Given `--cloudflare-token` 或 `--cloudflare-token-stdin` 已提供，
  When 运行 `xp-ops deploy --dry-run ...` 或发生 preflight 失败，
  Then 输出中不得出现 token 原文，且仍能定位 token 来源（例如 “provided via flag/env/file”）。

- Given token 通过命令行参数提供（`--cloudflare-token`），
  When `xp-ops deploy ...` 成功完成部署，
  Then 必须在结束时输出一条安全提示：建议轮换/废弃该 token（不输出 token 原文）。

- Given 未提供命令行 token 参数且未设置 `CLOUDFLARE_API_TOKEN` 且 token 文件不存在或为空，
  When 运行 `xp-ops deploy --cloudflare ...`，
  Then 命令以非 0 退出码失败，并给出可操作错误提示：指出可用的 token 传入方式（CLI 参数 / env / 文件）。

- Given 同时提供命令行 token 参数与 `CLOUDFLARE_API_TOKEN`，
  When 运行 `xp-ops deploy --cloudflare ...`，
  Then 以命令行 token 为准（优先级行为可通过测试验证），且输出不泄露 token。

## 8) 实现前置条件（Definition of Ready / Preconditions）

- 已冻结：命令行参数名为 `--cloudflare-token`，并额外支持从 `stdin` 读取 token（见 `contracts/cli.md`）。
- 已冻结：preflight config 中对 token 的展示口径（仅显示来源/是否提供，不显示原文）。

## 9) 测试与质量门槛（Non-functional）

### Testing

- Unit tests: 覆盖 token 来源优先级（flag/env/file）、token 缺失时的错误文案、以及“输出不泄露 token”（至少验证 preflight 输出不包含原文）。

### Quality checks

- Rust：保持仓库既有门槛：`cargo fmt`、`cargo clippy -- -D warnings`、`cargo test`。

## 10) 文档更新（Docs to Update）

- `docs/ops/cloudflare-tunnel.md`: 增加 “从命令行传入 token” 的示例与安全提示；并说明与 `sudo -E` / `CLOUDFLARE_API_TOKEN` / token 文件的优先级关系。
- `docs/ops/README.md`: 更新 token 获取与推荐用法小节（避免造成“只能靠 env/file”的误解）。
- （如需保持单一权威契约）`docs/plan/0013:cloudflare-tunnel-remote-access/contracts/cli.md`: 在实现合并后同步更新 `xp-ops deploy` 命令形状（仅更新与本计划新增参数相关的部分）。

## 11) 假设（已确认）

- `xp-ops deploy` 支持 `--cloudflare-token` 与 `--cloudflare-token-stdin`，并且 **不** 因此自动写入 `/etc/xp-ops/cloudflare_tunnel/api_token`（仅用于本次运行；是否落盘由 `xp-ops cloudflare token set` / TUI 保存负责）。

## 12) 参考（Repo reconnaissance）

- CLI 入口与参数：`src/ops/cli.rs`（`DeployArgs`）
- Deploy 预检与派生逻辑：`src/ops/deploy.rs`（`build_plan()`，当前使用 `load_cloudflare_token_for_deploy()` 解析 token + 来源）
- token 加载逻辑：`src/ops/cloudflare.rs`（`load_cloudflare_token_for_deploy()`）
- 现有运维文档：`docs/ops/cloudflare-tunnel.md`、`docs/ops/README.md`
