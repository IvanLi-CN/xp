# Web 原地升级入口历史（#nq4ha）

- 2026-07-04: 从 `#n5mtq` 的版本检查入口与 `#ap63t` 的统一 `xp-ops upgrade` 入口延伸，冻结当前节点 Web 触发原地升级范围。
- 2026-07-04: 决定第一版只支持 host-managed systemd/OpenRC；Docker/Compose 节点显示
  unsupported，并继续走宿主侧 image/Compose 升级。
- 2026-07-04: 落地受限 request/status 文件、admin upgrade API、`xp-ops _upgrade-runner`、
  systemd/OpenRC one-shot root 委托与 `VersionIndicator` popover。
- 2026-07-04: 补充共享测试机高成本回归脚本，覆盖 Web request/status 到
  `_upgrade-runner` 再到 `xp-ops upgrade` 的桥接路径。
