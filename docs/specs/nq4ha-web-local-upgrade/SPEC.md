# Web 原地升级入口（#nq4ha）

## 状态

- Status: 已完成
- Created: 2026-07-04
- Last: 2026-07-04

## 背景 / 问题陈述

- `#n5mtq` 已经让 Web 顶栏展示当前 `xp` 版本并检查 stable GitHub Release，但更新发现后的行动仍需要离开 UI 手动执行。
- `#ap63t` 已经把升级能力统一到 `xp-ops upgrade`，包含 release 锁定、`xp-ops` 自升级续跑、`xp` 替换、`xray` 静态配置收敛与失败回滚。
- 需要在不让 `xp` 以 root 运行、不复制升级逻辑、不扩大任意命令执行面的前提下，让 host-managed 当前节点能从 Web 触发一次受限原地升级。

## 目标 / 非目标

### Goals

- 顶栏版本入口收敛为单个 `VersionIndicator`，有更新时右侧图标切换为升级/下载图标。
- 版本入口支持 hover、focus、click 打开交互 popover，展示当前版本、最新版本、release 链接、检查状态、升级支持状态与最近 job 状态。
- 新增 admin-only 本机 upgrade job API：查询支持状态和最近 job，按 UI 确认后的 target tag 启动升级。
- 通过 `xp-ops` 隐藏 runner 和一次性 root 委托入口触发现有 `xp-ops upgrade`，不在 `xp` 内重写升级流程。
- 升级状态持久化到本机数据目录，`xp` 重启后 Web 仍能恢复 running/succeeded/failed/unsupported 状态。
- 明确 host-managed systemd/OpenRC 支持 Web 自动升级；Docker/Compose 节点不支持容器内自动升级，并显示宿主侧升级指引。

### Non-goals

- 不做全集群滚动升级。
- 不做远程节点选择或跨节点升级。
- 不让 `xp` 常驻 root。
- 不新增任意 shell 执行 API。
- 不提供 prerelease/channel 选择 UI。
- 不为 Docker/Compose 节点提供容器内自动升级。

## 范围（Scope）

### In scope

- `GET /api/admin/upgrade/status`
- `POST /api/admin/upgrade/start`
- `src/upgrade_job.rs` 持久化 job 状态与支持检测。
- `xp-ops _upgrade-runner` 隐藏入口。
- `xp-ops init` 写入 systemd/OpenRC 一次性 root 委托资产。
- `web/src/components/VersionIndicator.tsx`、admin upgrade API schema、Storybook 状态画廊。
- `docs/ops/**`、`AGENTS.md` 与本 spec 同步。

### Out of scope

- release channel 管理。
- 远端节点认证/调度协议。
- Docker image pull / Compose restart 自动化。
- Web 回滚按钮。

## 需求（Requirements）

### MUST

- `GET /api/admin/upgrade/status` 必须要求 admin auth，并返回：
  - `support.supported`
  - `support.reason`
  - `support.trigger` (`systemd|openrc|null`)
  - 最近 job status。
- `POST /api/admin/upgrade/start` 必须要求 admin auth，并要求请求体携带确认后的 `target_tag`。
- 当已有 active job (`running|restarting`) 时，重复启动必须返回 `409 upgrade_already_running`。
- 当运行环境是 Docker/Compose 容器时，Web 自动升级必须返回/展示 unsupported，不得在容器内替换自身。
- 一次性 root 委托只能触发固定 upgrade runner，不能传入任意命令。
- runner 必须读取 `XP_DATA_DIR` 下受限 request，调用现有 `xp-ops upgrade --version <target>`
  流程，并写 durable status。
- UI 点击 Upgrade 必须先弹出确认框；确认后才调用 start API。
- UI 必须在 running/restarting 时禁用重复启动，并轮询恢复状态。

### SHOULD

- Popover 内提供手动 Check 与 Status refresh。
- 失败状态应显示简短安全摘要，避免泄露 token/secret。
- host-managed systemd/OpenRC 样例应能直接解释 root 委托边界。

## 接口与运维契约（Interfaces & Ops Contracts）

### HTTP API

| Method | Path                        | Auth  | Behavior                                      |
| ------ | --------------------------- | ----- | --------------------------------------------- |
| GET    | `/api/admin/upgrade/status` | admin | 返回支持状态与最近 job                        |
| POST   | `/api/admin/upgrade/start`  | admin | 按 `target_tag` 启动当前节点升级；单 job 互斥 |

`POST /api/admin/upgrade/start` 请求体：

```json
{
  "target_tag": "v0.3.0"
}
```

The upgrade source repo is server-controlled from `XP_OPS_GITHUB_REPO`; the start API must not trust
or persist a browser-supplied repo override.

### Durable files

Under `${XP_DATA_DIR}`:

- `upgrade/request.json`: Web start API 写入的受限请求。
- `upgrade/status.json`: runner 与 `xp` 共同读写的最近状态。

Status states:

- `idle`
- `running`
- `restarting`
- `succeeded`
- `failed`
- `unsupported`

### Root delegation

- systemd: `xp-upgrade.service` 是 root one-shot service，`xp` 用户只能通过窄 polkit rule start 该 unit。
- OpenRC: `xp-upgrade` 是 root one-shot service，`xp` 用户只能通过窄 doas rule 执行
  `rc-service xp-upgrade start`。

## 验收标准（Acceptance Criteria）

- Given `update_available`，When 查看版本元素，Then 右侧显示升级/download icon。
- Given 版本元素获得 hover/click/focus，
  When popover 打开，
  Then 可看到 current/latest/release 链接、upgrade support 与最近 job 状态。
- Given 点击 Upgrade，When 未确认，Then 不调用 start API；When 确认，Then 调用 `POST /api/admin/upgrade/start`。
- Given 未带 admin auth，When 调用 upgrade status/start，Then 返回 401。
- Given active job 已存在，When 再次 start，Then 返回 409。
- Given Docker/Compose runtime，When start upgrade，Then 返回 unsupported，UI 显示不支持 Web 自动升级与宿主侧操作方向。
- Given `xp` 重启后读取同一 `XP_DATA_DIR`，When 查询 status，Then 最近 succeeded/failed/running 状态仍可恢复。
- Given systemd/OpenRC 委托入口，
  When 审计脚本/unit/doas/polkit，
  Then 只能触发固定 `xp-ops _upgrade-runner`，不能执行任意命令。

## 实现前置条件（Definition of Ready / Preconditions）

- 升级范围为当前连接节点。
- 升级目标默认 stable latest，由现有 `/api/version/check` 提供。
- 受限 root 委托形态为一次性 service/runner。
- Docker/Compose v1 不支持 Web 自动升级。

## 非功能性验收 / 质量门槛（Quality Gates）

- `cargo fmt`
- `cargo test`
- `cargo clippy -- -D warnings`
- `cd web && bun run lint`
- `cd web && bun run typecheck`
- `cd web && bun run test`
- `cd web && bun run storybook`
- `cd web && bun run test-storybook`
- UI 视觉证据：Storybook `Components/VersionIndicator/UpdateAvailable` 展示打开的 popover。

## Visual Evidence

- Storybook `Components/VersionIndicator/UpdateAvailable`
  - PR: include
  - ![Version indicator update available](./assets/version-indicator-update-available.png)
- Storybook `Components/VersionIndicator/UpdateAvailableUnsupported`
  - PR: include
  - ![Version indicator unsupported latest](./assets/version-indicator-unsupported-latest.png)

## 文档更新（Docs to Update）

- `docs/specs/README.md`
- `docs/ops/README.md`
- `docs/ops/systemd/xp-upgrade.service`
- `docs/ops/systemd/xp-upgrade.polkit.rules`
- `docs/ops/openrc/xp-upgrade`
- `docs/ops/openrc/doas-xp-upgrade.conf`
- `AGENTS.md`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 建立 owning spec，链接 `#n5mtq` 与 `#ap63t` 行为事实
- [x] M2: 后端 upgrade job 状态模型、admin API、单 job 并发保护与支持检测
- [x] M3: `xp-ops` 隐藏 runner 与 systemd/OpenRC 一次性 root 委托入口
- [x] M4: 顶栏 `VersionIndicator`、popover、确认框、升级轮询、API schema/tests、Storybook 状态画廊
- [x] M5: ops/AGENTS/spec 文档同步与验证证据

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：Web start API 成功触发 one-shot 后，`xp` 可能因升级重启导致短暂连接中断；UI 通过 status 轮询与版本检查恢复。
- 风险：host-managed 节点若未安装 `xp-ops init` 写入的 root 委托资产，会返回 trigger failure，需要操作者重新运行 init/deploy 路径。
- 假设：admin token 已保护 Web 管理面；upgrade start API 不额外引入第二套凭据。
- 开放问题：None.

## 变更记录（Change log）

- 2026-07-04: 创建并完成 Web 当前节点原地升级入口规格；实现委托 runner、admin API、顶栏 popover 与文档同步。
- 2026-07-05: 修复 systemd polkit private directory 对 delegate 检测的误判，同时拒绝只装 unit
  的半安装状态；unsupported 状态下 Web 升级按钮变为不可用展示，版本号统一显示 release tag
  风格，并稳定 popover hover 关闭行为。

## 参考（References）

- `docs/plan/n5mtq:ui-version-check/PLAN.md`
- `docs/plan/ap63t:xp-ops-upgrade-unified/PLAN.md`
- `docs/specs/c8qtw-docker-single-image-cluster-node-deploy/SPEC.md`
- `src/ops/upgrade.rs`
