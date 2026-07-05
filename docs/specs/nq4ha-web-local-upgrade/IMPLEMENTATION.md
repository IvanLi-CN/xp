# Web 原地升级入口实现（#nq4ha）

## 实现摘要

- `src/upgrade_job.rs` 定义本机 upgrade request/status 持久化模型、状态读写、host/container 支持检测与 start API helper。
- `src/http/mod.rs` 新增 admin-only `GET /api/admin/upgrade/status` 与 `POST /api/admin/upgrade/start`。
- `xp-ops _upgrade-runner` 读取 `${XP_DATA_DIR}/upgrade/request.json`，调用既有
  `xp-ops upgrade` 流程，并把 running/succeeded/failed 状态写回
  `${XP_DATA_DIR}/upgrade/status.json`。
- `xp-ops init` 为 host-managed systemd/OpenRC 写入一次性 root 委托入口：
  - systemd: `xp-upgrade.service` + 窄 polkit rule。
  - OpenRC: `xp-upgrade` one-shot script + 窄 doas rule。
- Web 顶栏改为单个 `VersionIndicator`，通过 Radix Popover 展示版本检查与升级状态；确认后
  调用 start API，并在 running/restarting 期间轮询 status。
- systemd 支持检测以 `xp-upgrade.service` 为委托安装信号；不要求 unprivileged `xp` 用户能读取
  `/etc/polkit-1/rules.d`，避免 CentOS/RHEL 系统上 private polkit 目录导致误报。
- Web unsupported 状态禁用升级确认入口，按钮文案为 `Unavailable`；版本展示统一按 release tag
  规范化为 `vX.Y.Z`，popover 使用延迟 pointer leave close，降低 polling 状态刷新造成的闪烁。

## 测试覆盖

- Rust:
  - upgrade status roundtrip、missing status idle、invalid target reject。
  - admin status auth gate、durable status recovery、active job 409。
  - `_upgrade-runner` 从 `upgrade/request.json` 读取 target，执行 mocked release upgrade，
    并把 `succeeded` durable status 写回 `upgrade/status.json`。
  - systemd unit/polkit 与 OpenRC doas policy 的窄触发测试。
  - systemd delegate 检测不依赖 polkit rules 文件对 `xp` 用户可读。
- Shared testbox live E2E:
  - `scripts/testbox/run-web-local-upgrade-live-e2e.sh` 在隔离共享测试机容器中启动真实
    `xp` 服务，通过 `POST /api/admin/upgrade/start` 触发升级，并用 fake systemd boundary
    只允许 `xp-upgrade.service` 与固定 restart 调用。
  - success case 从 `XP_BUILD_VERSION=0.2.0` 升级到 `0.2.1`，验证
    `/api/cluster/info` 返回新版本，并确认升级前创建的用户仍可读取。
  - rollback case 故意让 `xp.service` restart 失败，验证 durable status 进入
    `failed`、旧版本 `xp` 继续提供服务、升级失败二进制被保留为 `xp.failed.*`。
  - migration smoke 覆盖 legacy state JSON、state/usage version skew recovery 与 legacy grants
    snapshot install migration。
- Web:
  - admin upgrade API schema parse。
  - `Components/VersionIndicator` Storybook 覆盖 idle/checking/update/unsupported/running/failed/
    up-to-date/check-failed 状态。

## 验证命令

- `cargo fmt`
- `cargo test`
- `cargo clippy -- -D warnings`
- `cd web && bun run lint`
- `cd web && bun run typecheck`
- `cd web && bun run test`
- `scripts/testbox/run-web-local-upgrade-e2e.sh`
- `scripts/testbox/run-web-local-upgrade-live-e2e.sh`

## 操作边界

- Web 自动升级仅支持 host-managed systemd/OpenRC 节点。
- Docker/Compose 节点必须从宿主侧更新 image tag/digest 并重启容器。
- `xp` 不持有 root 权限；root 权限只存在于固定 one-shot runner。
