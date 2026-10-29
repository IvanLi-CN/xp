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
- 完成登录页节点选择器、公共候选探测、冷启动主后端恢复、认证成功后的持久化门禁，以及
  401 保留登录状态的测试与受控视觉证据。
- 收敛修复批次：登录提交等待公共探测与 mutation barrier，静态控制台保留 bootstrap transport，
  失败候选可重试，未知写入结果显式提示，并补齐移动端边距视觉证据。
- 处理 stale profile：当前 origin 的公共探测确认集群身份变化时，只重绑当前 tab 的内存状态，
  丢弃旧集群候选并延迟到 authenticated inventory 后再持久化。
- 收口认证启动边界：AppShell 先完成当前挂载的公开 cluster identity，再启动带 token 的
  compatibility 请求；inventory query 按 cluster 隔离，过期 probe 不能污染新 profile。
- 补齐认证请求门禁：旧 cluster-info 缓存不能反向恢复 stale profile，inventory 与 compatibility
  请求共同等待当前 origin 的公开 identity readiness，登录页探测在卸载或切换 epoch 后失效。

## 2026-10-29

- 收口跨集群认证边界：已认证公开探测发现 cluster identity 变化时，清除 token、恢复页面或
  static bootstrap transport 并回到登录页，禁止向新集群发送旧 Bearer token。
- 切换 primary backend 时取消并清理查询缓存；登录验证响应绑定 origin、cluster 和 generation，
  selector 在验证期间锁定；登出会清除 static runtime-policy transport。
- 补齐身份边界：mutation barrier 结束后重新校验 cluster/generation，资源导航等待公开 identity
  readiness，登出和 backend generation 变化清理内存及持久化离线查询缓存；认证 token 校验期间
  若节点切换则不保存 token。
- 补齐 Storybook 的冷启动 identity 夹具隔离与公共探测无 Authorization 的浏览器断言。