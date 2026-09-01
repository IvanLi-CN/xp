# 服务监控（Service Monitoring）主题历史

> 本文只记录主题局部的兼容性与背景；规范正文以 `./SPEC.md` 为准。

## Lifecycle / Compatibility

- 主题以 slug-only `uptime-monitoring` 新增。
- v1 复用 Cluster History Repository 的 stream、cursor、ready、quality 与 retention 合同。
- 新增 HTTP API、`service_monitor_observation-v1` 与 SQLite uptime schema。
- 不改变既有 endpoint probe 和节点局部历史行为。

## Replacements / Background

- 采集暂停、不变 revision/不补跑、Repository-first 持久化分别由
  ADR 0006、0007、0008 冻结。
- 未来 Incident/告警应扩展读模型，不能把告警状态写回 Observation outcome。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
- `./CONTEXT.md`
