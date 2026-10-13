# Mihomo 外部资源镜像实现覆盖

## Current Status

- Implementation：已有实现，并已补齐总时限实现与关键流边界测试。
- Lifecycle：active。
- 此文档记录源码与测试的覆盖事实；当前候选已运行 Rust/Web 定向与全量测试，最终交付仍以 PR CI 为准。

## Implementation Coverage

| 需求                     | 现有实现                                                      |
| ------------------------ | ------------------------------------------------------------- |
| REQ-MIRROR-001、004、005 | `src/http/mod.rs` 的订阅 query、主配置渲染与 origin 解析      |
| REQ-MIRROR-002、003、004 | `src/subscription.rs` 的外部资源改写与 GeoX 补齐              |
| REQ-MIRROR-006           | `src/mihomo_resources.rs` 的 HMAC ID/目录与 HTTP 目录版本复核 |
| REQ-MIRROR-007、009      | `src/mihomo_resources.rs` 的流式转发、限额和错误映射          |
| REQ-MIRROR-008           | `src/mihomo_policy.rs` 与资源转发前的地址过滤/固定            |
| REQ-MIRROR-010           | `UserDetailsPage`、`SubscriptionPreviewDialog` 与共享镜像开关 |
| REQ-MIRROR-011           | XP 内的共享 HTTP 路由与节点本地策略，部署路径见 ops 文档      |

已有测试覆盖：

- `src/subscription/tests.rs`：资源重写、GeoX 默认补齐、HTTP 与 Header 拒绝。
- `src/http/tests.rs`：canonical 镜像输出、无 profile 拒绝、raw 格式拒绝与未知资源 ID。
- `src/mihomo_resources.rs`：资源 ID、目录版本、内容流式转发、错误体脱敏、重定向上限，以及请求超时 `504`、统一截止时间和响应头后的流中止。
- `src/mihomo_policy.rs`：节点策略文件、环境默认与 CIDR 校验。
- `web/src/views/UserDetailsPage.test.tsx`：镜像开关只作用于 Mihomo 预览请求。

## Remaining Gaps

- 部署合同检查和 PR CI 仍是交付门禁；本地测试通过不替代这些检查。
- 真实节点、外部 GeoX 上游和生产部署未纳入本地测试；受控上游只验证 XP 的转发合同。

## References

- [长期需求](./SPEC.md)
- [主题历史](./HISTORY.md)
- [部署合同](../../ops/README.md)
