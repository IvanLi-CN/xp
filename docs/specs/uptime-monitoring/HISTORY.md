# 服务监控（Service Monitoring）主题历史

> 本文只记录主题局部的兼容性与背景；规范正文以 `./SPEC.md` 为准。

## Lifecycle / Compatibility

- 主题以 slug-only `uptime-monitoring` 新增。
- v1 复用 Cluster History Repository 的 stream、cursor、ready、quality 与 retention 合同。
- 新增 HTTP API、`service_monitor_observation-v1` 与 SQLite uptime schema。
- 不改变既有 endpoint probe 和节点局部历史行为。
- Observer Policy 兼容旧 `observer_node_ids`，并以独立的短期 Draft Cluster Test
  记录创建前的集群证据；草稿结果不进入 Observation 或长期历史。
- Draft Cluster Test 由协调 Leader 保存短期 runtime，但浏览器访问任何 follower 时仍
  通过同源、签名的服务端 forwarding 创建和读取；该边界由 ADR 0010 固定。

## Replacements / Background

- 采集暂停、不变 revision/不补跑、Repository-first 持久化分别由
  ADR 0006、0007、0008 冻结。
- Observer Policy、临时测试和同源转发分别由 ADR 0009、0010 冻结。
- 未来 Incident/告警应扩展读模型，不能把告警状态写回 Observation outcome。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
- `./CONTEXT.md`
