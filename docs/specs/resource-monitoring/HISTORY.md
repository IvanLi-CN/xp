# Resource Monitoring history

> 这里记录主题局部生命周期、替换、兼容性与必要背景；完整 ADR 取舍保留在 `docs/adr/`。

## Compatibility

- Resource Monitoring 是对现有节点运行态与 History Repository 的 additive 能力；旧节点不支持时保持原有 API 行为，新客户端明确显示
  capability，而不是把缺失值解释为零。
- `resource_metrics.sqlite3` 是独立的本地 Resource Store。host-managed 升级和 Docker/Compose 升级都必须保留它；
  没有历史回填。

## Replaced approaches

- 本主题取代“通过运行状态、连接数或发布验收脚本间接判断资源压力”的做法；它不取代 Service Monitoring、Traffic Monitoring 或 Uptime
  Monitoring。
- 资源历史使用 History Repository 的有界交付机制，但采用专用数值 reducer，避免通用 reducer 丢失资源语义。

## Failure diagnostics

- Node Details resource reads use a typed failure contract so circuit cooldown, transport
  uncertainty, protocol rejection, verified remote application errors, XP API failures, and
  browser offline state remain distinguishable without exposing transport internals.
- The current-page snapshot is a view fallback only. It is not added to offline persistence, and its
  observed time, last successful fetch time, and stale age remain separate from the failure state.
