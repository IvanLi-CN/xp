# GitHub 仓库敏感信息清理（历史重写）运行手册

本手册用于处理“敏感信息被提交进 Git 历史并已推送到 GitHub”的事故场景：通过**最小范围的历史重写**移除泄露内容，并给出 GitHub 侧的清理/通知动作清单。

> 适用范围：本仓库为开源项目，任何已推送到远端的提交都可能已被 fork / clone / 缓存；因此必须把“技术清理 + 社区同步”作为一个整体流程。

## 定义

- **敏感信息**：包括但不限于生产域名/内网地址、真实节点名、真实用户标识、访问令牌、密钥、凭据、生产配置片段、可用于定位生产环境的唯一信息。
- **清理**：从 GitHub 上“可达引用（branches/tags）”中移除敏感内容；对 GitHub 缓存/索引与 fork 的残留需要额外处理。

## 事故处理目标

1. 从“引入问题的提交”开始，重写之后的历史，确保敏感内容不再存在于任何将被推送的 refs（`main`、受影响分支、受影响 tags）里。
2. 不改变更早的历史（引入提交之前的 commit 及其 hash 保持不变）。
3. 强制推送更新后的分支与标签到 GitHub。
4. 给出 GitHub 侧与协作者侧的后续动作，降低缓存/索引/fork 的残留风险。

## 推荐流程（维护者）

### 1) 立刻止血（流程层面）

- 暂停合并与发布（保护 `main`，暂停 CI 自动发布）。
- 如涉及凭据/令牌：**先旋转（rotate）**，再做历史清理（避免“清理未完成期间继续被滥用”）。

### 2) 定位引入点（最小范围）

- 目标：找出**第一次把敏感字符串写入仓库**的提交（称为 `BAD_COMMIT`）。
- 常用做法：
  - `git log -S '<敏感子串>' -- <path>`
  - `git blame <path>` 定位引入行

### 3) 本地备份（只在本地，不推送）

- 为当前远端状态打本地引用，便于回滚：
  - `backup/pre-sanitize-main`
  - `backup/pre-sanitize-<branch>`
  - `backup/pre-sanitize-<tag>`

> 注意：备份引用本身也“保留了敏感内容”，因此**绝对不要推送**到 GitHub。

### 4) 历史重写（仅从 `BAD_COMMIT` 开始）

本仓库约束是：**只允许重写 `BAD_COMMIT` 及其之后的历史**，更早的 commit 不得变化。

推荐实现方式（可复现、可控）：

- 以 `BAD_COMMIT^` 作为基线，把 `BAD_COMMIT..HEAD` 的提交用 `cherry-pick` / `rebase -i` 重放出来；
- 在重放到 `BAD_COMMIT` 时，直接把敏感内容替换为安全占位符：
  - 域名：使用 `*.example.invalid`
  - ID：使用伪造/随机的不可关联值
  - 绝不写入生产真实数据

### 5) 验证（推送前必须做）

- 在将被推送的 refs 上做扫描（只扫将推送的分支/标签，避免被本地备份干扰）：
  - `git grep -n -E '<pattern>' <ref> -- <path>`
  - `git log <ref> -S '<pattern>' -- <path>`
- Storybook / 测试数据必须满足：
  - 只用 mock（不得引用生产 API / 生产地址）
  - 不包含可识别生产环境的域名、节点名、账号、ID

### 6) 强制推送到 GitHub（分支 + 标签）

- 受影响分支：`git push --force-with-lease origin <branch>`
- 受影响标签：`git push --force origin refs/tags/<tag>`

> 风险提示：历史重写会让所有协作者本地分支与远端发生不可快进（non-fast-forward）分歧。

### 7) GitHub 侧“残留清理”与通知

即使 refs 已不可达，GitHub 仍可能在一段时间内保留对象/缓存/搜索索引；此外 fork 也可能继续公开旧历史。

- **通知协作者/用户**（README/Release/Discussion）：
  - 本仓库发生历史重写
  - 需要重新同步（见下节“协作者自救”）
- **处理 fork**：
  - 对关键 fork 逐个通知：建议删除并重新 fork，或强制同步上游
- **请求 GitHub 清理缓存**（如泄露影响较大）：
  - 参考 GitHub 官方文档 “Removing sensitive data from a repository”
  - 如必要：向 GitHub Support 提交 “purge cached views / search indexes / git objects” 的请求

## 协作者自救（历史已重写后的同步方式）

若本地没有未推送的工作：

```bash
git fetch --all --prune
git checkout main
git reset --hard origin/main
```

若本地有未推送提交（不要丢）：

1. 先用 `git format-patch` 或 `git branch backup/my-work` 备份本地提交
2. 再执行上面的 `reset --hard`
3. 最后把备份的提交用 `cherry-pick` 重新应用到新历史上

## 本仓库的额外约束（必须长期遵守）

- Storybook、测试夹具、示例配置：一律使用不可解析的保留域名（推荐 `example.invalid`），不得出现生产真实信息。
- 若需要展示“看起来像真的”的示例：只做形状模拟，不做可关联/可定位的真实数据模拟。

