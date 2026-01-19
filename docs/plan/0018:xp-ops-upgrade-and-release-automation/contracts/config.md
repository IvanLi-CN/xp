# Config Contracts（#0018）

本文件用于冻结 `xp-ops` 在“从 GitHub Releases 拉取升级产物”时使用的输入口径（repo、API base）。

## GitHub repo selection

- 默认 repo（normative）：`IvanLi-CN/xp`
- 覆盖方式（实现阶段二选一并固化，推荐同时支持）：
  - CLI：`--repo <owner/repo>`
  - Env：`XP_OPS_GITHUB_REPO=<owner/repo>`

若两者同时存在，CLI 优先生效。

## GitHub API base

- Env：`XP_OPS_GITHUB_API_BASE_URL`
  - 默认：`https://api.github.com`
  - 用途：测试/自建 GitHub API 兼容端点（CI mock）
