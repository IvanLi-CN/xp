# Implementation

## Current State

- The user Mihomo mixin and provider-template behavior in `SPEC.md` is implemented.
- Current subscription output remains defined by the live template and API contracts.

## Recorded Delivery State

- Status: 已完成
- Created: 2026-03-04
- Last: 2026-04-24

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 状态层新增 `UserMihomoProfile` 持久化与 Raft 命令
- [x] M2: HTTP 管理 API + 订阅 `format=mihomo` 路由落地
- [x] M3: Mihomo 渲染引擎（系统节点生成 + 合并 + 冲突重命名 + relay use 注入）
- [x] M4: Web UserDetails 编辑 + `mihomo` 预览
- [x] M5: 测试补齐与质量门禁通过
- [x] M6: 管理 API 主字段切换为 `mixin_yaml`，并移除旧字段 `template_yaml` 兼容层
- [x] M7: 订阅渲染与后续 provider-only 合同对齐，mixin 不要求包含系统动态组定义
- [x] M8: provider 为空 / extra proxies 保留 / 共享测试机 Mihomo 校验与脱敏输出证明
