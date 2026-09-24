# History

- The topic originated in legacy Spec `#9vmap`.
- It consolidates the completed node-service observability work under a slug-only topic.

## 变更记录（Change log）

- 2026-02-26: 创建规格并冻结首版接口、状态枚举、窗口策略。
- 2026-02-26: 完成后端运行态聚合/持久化、前端 Nodes/NodeDetails 改造、cloudflared 运维配置与文档同步。
- 2026-04-24: 在节点 metadata API 与 `NodeDetailsPage` 补充主动探测 egress probe 摘要、单节点刷新入口与 Storybook 视觉证据。
- 2026-05-16: 明确 DDNS 地址族缺失不等同于探测异常，无 IPv6 出口节点不应仅因 IPv6 probe 失败进入 degraded。
