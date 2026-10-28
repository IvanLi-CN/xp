# Web 主后端切换演进历史

> 长期行为合同见 `./SPEC.md`；此处仅保留主题生命周期和兼容性背景。

## Decision Trace

- 采用嵌入式 PWA 的单主后端手动切换，暂不引入独立静态部署。
- 采用当前注册节点的精确 origin allowlist，避免任意 URL 和跨集群 token 泄露。
- 保留既有全页节点导航作为兼容路径，不将其改造成浏览器直连多节点控制面。
- 独立静态 Web 采用 EdgeOne Makers `xp-web` 直接上传；发布先通过 `xp.ivanli.cc` 的公开静态门禁，
  再发布镜像与 GitHub Release。运行时节点 origin 通过受保护的 runtime-policy 和逐客户端
  Service Worker 导航 CSP 授权，不引入匿名拓扑发现、Edge Function 或 API 代理。

## Compatibility

- API 兼容窗口与 PWA build ID 继续独立。
- 已有相对 API 调用、SSE、离线读缓存和 mutation 写保护继续有效。

## 2026-10-28

- 确认登录页的 Primary Backend 恢复缺口：认证失败时，AppShell 内的节点切换器不可达。
- 收敛登录前恢复合同：只使用当前 origin 与活动集群 profile 中已记录的候选，拒绝任意 URL
  和匿名节点清单发现。
- 要求冷启动恢复持久化 `primaryOrigin`，并让选中候选成为当前 tab 的 token 验证目标；认证成功
  前不持久化新的主后端。
- 新增 ADR 0028，记录预认证恢复边界及同集群 admin token 一致性约束。
