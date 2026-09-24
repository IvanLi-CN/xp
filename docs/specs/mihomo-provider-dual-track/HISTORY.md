# History

- The topic originated in legacy Spec `#3e4q4`.
- It preserves the dual-track subscription contract after the legacy ID-based catalog was retired.

## 变更记录（Change log）

- 2026-04-17: 创建规格并冻结双轨 URL、provider 保留名与双轨 admin 设置语义。
- 2026-04-17: 完成全局 `mihomo_delivery_mode`、显式 dual-track 路由、Storybook/真实 Mihomo provider 验证与文档同步。
- 2026-04-24: provider 主配置的系统地区组切换为 probe-derived 固定地区面，并补充 `🌟 Other`、`💎 高质量` / `🚀 节点选择` 自动补点语义。
- 2026-05-02: 冻结 provider-only Mihomo 口径；系统 provider 动态输出直连与链式节点，主配置通过 provider filter 消费链式候选。
- 2026-05-02: 修正 provider-only 高质量与地区接入点口径；`🔒 高质量` / `🔒 {Region}` 动态包含系统 `{base}-reality`，`🛬 {base}` 通过 system provider payload 顺序保证 ss-chain 先于 reality-chain。
- 2026-06-14: relay 外层中转从共享 `🛣️ JP/HK/SG` 改为按 `Node.access_host` 聚合的 per-base relay 组，并删除 `🛣️ {Region}` 兼容地区别名。
- 2026-06-15: Mihomo profile 保存收紧为“预渲染联合校验 + 明确 invalid_request”；移除静默 remap / prune / autosplit，hidden relay 组统一移到系统托管组尾部。
- 2026-06-15: 明确补充高质量入口兜底合同；`💎 高质量` 之上必须存在稳定的全局兜底聚合入口，不能因 mixin 缺失或系统组收敛而消失。
- 2026-06-16: relay 外层健康检查切到托管 VLESS 端口自身的 HTTPS canary（`https://<access_host[:port]>/generate_204`），并冻结地区组合同为：`🌟 {Region}` 是 hidden source group，`🔒 {Region}` 是 owner-facing 可见入口，`🤯 {Region}` 是 hidden `url-test` 包装组。
- 2026-06-23: 托管 VLESS 的 SNI/dest 收敛为固定协议事实：`server_names=[node.access_host]`、`dest=XP_VLESS_CANARY_BIND`；canary 增加 endpoint-level `canary_upstream` 透明 TLS 终止反代，并通过 HTTP authority 匹配 endpoint。
- 2026-06-28: 托管 VLESS canary 增加 endpoint-level `accepted_authorities` 无序 `host[:port]` 别名集合；省略端口时按 HTTPS 默认 `443` 解释。公共未命中和未配置 upstream 的表面统一收敛为普通纯文本 `404 Not Found`，不暴露内部协议和匹配细节。
