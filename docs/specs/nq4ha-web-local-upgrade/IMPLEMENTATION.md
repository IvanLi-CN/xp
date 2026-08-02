# Web 原地升级入口实现（#nq4ha）

## 实现摘要

- `src/upgrade_job.rs` 定义本机 upgrade request/status 持久化模型、状态读写、host/container 支持检测与 start API helper。
- `src/http/mod.rs` 新增 admin-only `GET /api/admin/upgrade/status` 与 `POST /api/admin/upgrade/start`。
- `xp-ops _upgrade-runner` 读取 `${XP_DATA_DIR}/upgrade/request.json`，调用既有
  `xp-ops upgrade` 流程，并把 running/succeeded/failed 状态写回
  `${XP_DATA_DIR}/upgrade/status.json`。
- `xp-ops init` 为 host-managed systemd/OpenRC 写入一次性 root 委托入口：
  - systemd: `xp-upgrade.service` + root-owned 固定 helper + 窄 sudoers drop-in；窄 polkit
    rule 作为新系统兼容补充。
  - OpenRC: `xp-upgrade` one-shot script + root-owned fixed readiness helper + two narrow
    doas rules for `--check` and fixed runner start。
- Web 顶栏改为单个 `VersionIndicator`，通过 Radix Popover 展示版本检查与升级状态；确认后
  先在 `sessionStorage` 写入目标版本与绝对截止时间，再调用 start API。观察器每 2.5 秒查询
  status，跨同标签页刷新只保留剩余时间。
- 无结构 5xx、网络中断和 `409 upgrade_already_running` 保留观察而不显示为确定失败；带 code 的
  结构化拒绝立即结束观察并展示 API 错误。status 的 succeeded/failed/unsupported 终态停止轮询并
  保持结果，直到用户主动关闭 popover。
- 观察期间 popover 维持打开并禁用 Upgrade，pointer leave 不会关闭它；用户仍可用点击外部或 Esc
  主动收起，顶栏 spinner 与后台轮询继续。60 秒无确定状态时进入 timeout、停止轮询并保持锁定；手动
  Status 查到 active job 时建立新窗口，查到 idle 或 terminal 时解除锁定。
- systemd 支持检测要求 `xp-upgrade.service` 存在，并验证以下任一窄授权：
  - `sudo -n /usr/local/libexec/xp-upgrade-trigger --check` 可成功执行，且
    `sudo -n -l /usr/local/libexec/xp-upgrade-trigger` 确认 no-arg start grant 存在。
  - 窄 polkit rule 可读并限定 `xp-upgrade.service` + `start`。
  - 当前进程通过 `pkcheck` 被授权 start 固定 unit。
- systemd 触发优先执行 `sudo -n /usr/local/libexec/xp-upgrade-trigger`。只有 helper 授权不可用时，
  才回退到 `systemctl start --no-block xp-upgrade.service` 的 polkit 路径。CentOS 7-class
  polkit 不可靠提供 `unit` / `verb` action detail，因此不能把 polkit 作为唯一 systemd
  Web upgrade 委托。
- OpenRC 支持检测先确认 runner/helper 资产，再执行
  `doas -n /usr/local/libexec/xp-openrc-upgrade-trigger --check`。helper 以 root 检查
  executable runner 与精确 start rule，因此 `xp` 无需也不得直接读取 root-only
  `/etc/doas.conf`；实际触发仍是 non-interactive fixed
  `doas -n /sbin/rc-service xp-upgrade start`。
- 已有 OpenRC 节点跨越 helper 引入版本时，root 在普通 `xp-ops upgrade` 完成后显式执行一次
  `xp-ops init` 安装 helper/policy。正在运行的旧版 `xp-ops` 无法执行新 release 中的回填逻辑，
  因此 status/readiness path 不得以启动 one-shot service 作为隐式迁移。
- systemd `xp-upgrade.service` 直接执行 `/usr/local/bin/xp-ops _upgrade-runner`，并通过 unit
  environment / `/etc/xp/xp.env` 给 runner 提供 `XP_DATA_DIR`。runner CLI 自身读取该环境变量并
  保留 `/var/lib/xp/data` 默认值，避免 systemd 在 `/bin/sh -c` 命令行中提前展开 shell 风格
  `${XP_DATA_DIR:-...}`。
- upgrade status 读取路径会对 active durable status 做本机自愈：当 systemd
  `xp-upgrade.service` 已明确进入 `failed` 状态，而 `status.json` 仍停在 `running` /
  `restarting`，`xp` 会把 durable status 写回 `failed` 并返回该失败给 Web UI。
- Web unsupported 状态禁用升级确认入口，按钮文案为 `Unavailable`；版本展示统一按 release tag
  规范化为 `vX.Y.Z`，popover 使用延迟 pointer leave close，降低 polling 状态刷新造成的闪烁。
- `/api/version/check` 保留 1 小时 latest-release 缓存；admin-authorized
  `/api/version/check?refresh=1` 和 `force=1` 不复用自动检查缓存，并在成功后更新缓存内容。
  `VersionIndicator` 的手动 Check 走 refresh 模式并携带 admin bearer token，自动 focus
  check 仍走公开的 1 小时缓存路径。

## 测试覆盖

- Rust:
  - upgrade status roundtrip、missing status idle、invalid target reject。
  - admin status auth gate、durable status recovery、active job 409。
  - `_upgrade-runner` 从 `upgrade/request.json` 读取 target，执行 mocked release upgrade，
    并把 `succeeded` durable status 写回 `upgrade/status.json`。
  - systemd unit/helper/sudoers/polkit 与 OpenRC helper/doas policy 的窄触发测试。
  - systemd upgrade unit 不再经过 shell `--data-dir` 展开；active durable status 遇到已失败
    one-shot 会收敛为 failed，无明确 delegate failure 时仍保持 active 并保留 409 并发保护。
  - systemd delegate 检测拒绝只安装 unit 的半安装状态；拒绝只允许 helper `--check`
    的不完整 sudoers，真实 root 探测同时验证 `--check` 与 no-arg start grant，并允许通过
    有效 helper 授权或 polkit 授权恢复支持判定。
- Shared testbox OpenRC delegate regression runs real Alpine OpenDoas as the `xp` user against a
  `0600 root:root` policy. It proves the fixed `--check` succeeds, does not call `rc-service`,
  rejects wrong arguments, and fails after the fixed start rule or helper is removed.
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
  - version check API schema/fetch tests cover cached default requests and user-forced refresh
    requests.
  - admin upgrade API schema parse。
- `Components/VersionIndicator` Storybook 覆盖 idle/checking/update/unsupported/running/failed/
  up-to-date/check-failed/reconnecting/timeout 状态，并用 `play` 覆盖确认后 popover 保持打开与
  Upgrade 锁定。
- `upgradeObservation.test.ts` 覆盖无结构 502 后持续观察、terminal 收口、60 秒 timeout、手动
  active 续期和 `sessionStorage` 恢复；`VersionIndicator.test.tsx` 覆盖确认后 popover 生命周期。

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
