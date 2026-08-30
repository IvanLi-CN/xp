# 服务监控（Service Monitoring）实现状态

> 当前规范见 `./SPEC.md`。这里记录代码覆盖、rollout 与剩余实现事实。

## Current Status

- Implementation: spec ready; implementation not started
- Lifecycle: active
- Catalog note: 先交付数据/执行面，再接 API 与 Web。

## Architecture

### Control plane

- 在 DesiredState/Raft 中增加 Monitor、revision、lifecycle、budget reservation
  与 capability summary。
- Leader 分配 ULID、revision 和 `effective_at`；Raft 不保存高频 Observation。
- PATCH 使用 `expected_revision`；删除是 lifecycle tombstone，不能 purge。

### Execution

- `src/uptime_monitor` 负责 schedule、executor、capability、outcome 和 status aggregation。
- slot idempotency key 是 monitor、revision、observer、scheduled_at、mode 的组合。
- HTTP client 关闭自动重定向，逐跳验证目的 URL；body 限制 64 KiB。
- PING 通过 Linux ICMP datagram socket 执行；raw socket 只作同语义 capability fallback。
- TCPING 通过 Tokio TCP connect 执行并立即关闭。

### Storage and sync

- `history.sqlite3` 增加 uptime raw、capture range、rollup 与索引表。
- Source 使用 `service_monitor_observation-v1` journal 分区，
  沿用 canonical payload 与签名链。
- raw 唯一键覆盖 monitor、observer、slot、mode；receiver 先幂等再进行 rollup。
- rollup 主键覆盖 monitor、revision、observer、resolution、bucket_start。
- bucket 保存 counters、count/sum/min/max、固定 histogram、errors、
  watermark、aggregate hash。
- child hash 完整时才允许 minute -> 5m -> 1h；
  清理与 completion marker 同一 transaction。

### API and UI

- `src/http` 增加 `/api/admin/monitors` 路由、错误 shape、history parser
  与 status/run handlers。
- `web/src/api` 增加 `adminMonitors.ts`；views 增加 overview、new、detail、edit。
- 注册 `/monitors`、`/monitors/new`、`/monitors/:monitorId` 与 edit 路由和导航。
- 复用 PageHeader、ResourceTable、ECharts、offline cache 与
  History Repository quality 组件。

## Rollout / migration

1. 添加 SQLite 迁移与 schema family；既有 history stream 不变。
2. 交付 executor 与 ICMP self-test，验证 systemd、OpenRC、Docker/Compose。
3. 启用 Raft config、read API 和 scheduler；检查 revision 与 budget rejection。
4. 用 canary monitor 检查 journal、quality、rollup、资源与 retention 边界。
5. 启用 Web 路由；API、Repository、UI E2E 通过后移除 feature guard。

## Remaining Gaps

- 固定 ICMP adapter 在支持内核和 capability 组合上的生产测试。
- 固定 API fixtures、histogram bucket 与迁移版本。
- 绑定最终 integration SHA、视觉证据和 PR 状态。

## References

- `./SPEC.md`
- `./HISTORY.md`
- `./contracts/api.md`
