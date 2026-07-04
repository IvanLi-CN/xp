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

## 测试覆盖

- Rust:
  - upgrade status roundtrip、missing status idle、invalid target reject。
  - admin status auth gate、durable status recovery、active job 409。
  - systemd unit/polkit 与 OpenRC doas policy 的窄触发测试。
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

## 操作边界

- Web 自动升级仅支持 host-managed systemd/OpenRC 节点。
- Docker/Compose 节点必须从宿主侧更新 image tag/digest 并重启容器。
- `xp` 不持有 root 权限；root 权限只存在于固定 one-shot runner。
