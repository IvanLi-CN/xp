# Web 运行时韧性实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Implementation: #226 framework error recovery 与 #227 PWA/API compatibility child PR 已由
  aggregate PR #230 合入 `main` 并发布为 `v3.23.0`。
- Lifecycle: active
- Catalog note: Initiative `web-runtime-resilience`

## Coverage / rollout summary

- 两个独立 Ticket child PR 已通过 aggregate PR #230 的集成验证与 review。
- #226 覆盖框架错误分类、恢复界面、静态缓存受保护操作、脱敏诊断、文档 fallback 与
  Storybook/mock-only `ui_demo` 证据。
- #227 覆盖版本化 PWA app-shell、waiting 更新确认、跨 tab ownership、失败安装恢复、
  无混合 build 资源，以及 3.22/3.21/3.20 API inventory/capability/version/fingerprint
  compatibility。
- 后续 legacy Workbox migration 修复把“旧 controller + orphan XP waiting cache + 无 ownership”纳入
  production preview 回归。完整 Worker 可后台激活，但不 claim 或刷新旧页；最终回收仅限 migration state
  记录的 cache。AppShell 的升级状态同步不再重置关联 mutation；直接返回 `succeeded`、`failed` 或
  `unsupported` 的升级响应均由端到端回归覆盖，管理页保持可用且无 React #185。已确认终态仅在用户关闭
  结果（含键盘关闭）时清除 start mutation，避免已被状态轮询确认的 network/5xx 错误重新显示。
- Service Worker 注册完成后先执行一次 `registration.update()`，再开始既有周期轮询；因此已运行旧
  bundle 的标签无需等待首个轮询周期即可发现 waiting worker，但仍保留显式 Reload 确认。
- 当受控公开导航发现 active app-shell 缺少 metadata 或预缓存资源时，Worker 保留原 cache，完整验证同一 build 的
  staged recovery cache 后从其交付登录入口；Playwright 覆盖该完整启动前故障，防止 raw 504 cache-miss 页面再次阻断登录。

## Remaining Gaps

- legacy migration 修复需随下一次 Web release 交付；正常 XP-to-XP waiting update 合同不受影响。

## Related Changes

- #226 child PR [#228](https://github.com/IvanLi-CN/xp/pull/228): merged as
  `5d0d930c0cc06530d10c5c78efdf34216ce5f83a`.
- #227 child PR [#229](https://github.com/IvanLi-CN/xp/pull/229): merged as
  `51dfd061241c67ece6e135ae2898f7b87efe26a8` after all required checks passed.

## References

- `./SPEC.md`
- `./HISTORY.md`
