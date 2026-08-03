# Web 原地升级入口历史（#nq4ha）

- 2026-07-04: 从 `#n5mtq` 的版本检查入口与 `#ap63t` 的统一 `xp-ops upgrade` 入口延伸，冻结当前节点 Web 触发原地升级范围。
- 2026-07-04: 决定第一版只支持 host-managed systemd/OpenRC；Docker/Compose 节点显示
  unsupported，并继续走宿主侧 image/Compose 升级。
- 2026-07-04: 落地受限 request/status 文件、admin upgrade API、`xp-ops _upgrade-runner`、
  systemd/OpenRC one-shot root 委托与 `VersionIndicator` popover。
- 2026-07-04: 补充共享测试机高成本回归脚本，覆盖 Web request/status 到
  `_upgrade-runner` 再到 `xp-ops upgrade` 的桥接路径。
- 2026-07-04: 补充 live shared-testbox 回归脚本，启动真实旧版 `xp`，通过 Web admin API
  触发升级，验证新版重启、失败自动回滚与关键迁移 smoke。
- 2026-07-05: hinet-lam 验证暴露 CentOS 7 `polkit` rules 目录默认不可由 `xp` 用户遍历；
  systemd delegate 支持检测收敛为 unit 存在加 polkit 授权验证，避免把有效委托误报为
  `missing installed upgrade delegate`，也避免只装 unit 的半安装被误判为 ready。
- 2026-07-30: OpenRC `doas.conf` 的 root-only 权限使服务用户直接读取 policy 产生 false
  negative。改由 root-owned fixed helper 执行 `--check`，以 doas 成功退出作为 readiness
  事实，并在 Alpine/OpenDoas `0600 root:root` fixture 中验证正反例。已有节点首次跨越该边界
  后由 root 显式运行 `xp-ops init` 安装新资产；不通过 status check 启动 one-shot service 迁移。
- 2026-07-05: `VersionIndicator` unsupported 状态改为不可触发升级的 `Unavailable` 操作，
  current/latest 版本展示统一为 `vX.Y.Z` release tag 风格，并延迟 hover close 以降低状态刷新造成的闪烁。
- 2026-07-05: hinet-lam Web upgrade start 暴露 CentOS 7 polkit 0.112 不提供
  `org.freedesktop.systemd1.manage-units` 的 `unit` / `verb` detail；原窄 polkit rule 永远不匹配，
  `xp` 用户触发 `systemctl start xp-upgrade.service` 会要求交互认证。systemd 委托改为优先
  root-owned 固定 helper + 窄 sudoers，polkit 只保留为兼容补充。
- 2026-07-06: hinet-lam 升级到 `v3.17.2` 时暴露 systemd unit 中的 shell 风格
  `${XP_DATA_DIR:-...}` 会被 systemd 命令行展开提前处理，导致 `_upgrade-runner --data-dir`
  收到空值并以 invalid argument 失败；同时 durable status 已写入 `running`，runner 失败前没有机会
  写回 terminal status。systemd unit 改为直接执行 `_upgrade-runner`，status API 增加 failed
  one-shot 自愈，避免 UI 永久显示 running。
- 2026-07-07: hinet-lam 在 `v3.17.4` 发布后仍显示 latest `v3.17.3`，根因是
  `/api/version/check` 在发布前缓存了旧 latest 一小时；手动 Check 改为 refresh 请求，保留自动检查缓存。
- 2026-08-03: SG 节点升级触发后，服务重启边界返回的无结构 502 会让旧 UI 停止轮询并关闭 popover，
  即使 durable job 已开始。客户端改为先持久化同标签页观察记录，持续查询 status 60 秒；只有服务端
  terminal state、结构化拒绝或 timeout 才终止观察，手动 Status 可依服务端事实恢复或解除锁定。
- 同日修复观察边界：残留 terminal status 不会结束新 attempt，且刷新恢复的 terminal result 不会被误报为 timeout。
