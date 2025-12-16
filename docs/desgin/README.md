# docs/desgin

本目录放置项目的核心设计文档与配套规范：

**核心（必读）**

- `requirements.md`：需求分析
- `tech-selection.md`：技术选型（前端/后端/工具链）
- `architecture.md`：架构设计（控制面/集群/下发/配额）
- `quality.md`：代码质量保证方案（Biome、单测、Storybook、CI 门禁）

**业务/协议细则（按需）**

- `xray.md`：Xray 集成（API、动态入站、统计约束）
- `quota.md`：配额与周期重置规则
- `subscription.md`：订阅输出规格（URI/Base64/Clash）
- `api.md`：HTTP(S) API 设计（对外契约）
- `cluster.md`：Raft + mTLS + HTTPS-only 集群细则
- `workflows.md`：操作流程与 reconcile 不变量
