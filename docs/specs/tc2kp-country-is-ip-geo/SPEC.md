# country.is Hosted IP Geo hard cut

## 背景

原先的入站 IP Geo 依赖本地 MMDB 文件与 DB-IP Lite 下载更新。该方案会占用本地磁盘，并在更新失败时留下临时文件，已经不适合作为默认方案。

## 目标

- 用免费、无鉴权、零门槛的 `country.is` Hosted API 替换本地 MMDB 方案。
- 删除 Geo DB 设置页、手动更新、托管下载与外部 MMDB override 能力。
- 保持 Node/User IP usage 继续输出 `country / region / city / operator`，并在 UI 中标注 `country.is` attribution。
- 复用同一 Geo 解析能力给节点主动探测出口 IP，作为订阅地区自动生成的唯一 Geo 真相源。

## 非目标

- 不保留 `/ip-geo-db` 只读页或兼容入口。
- 不继续支持 `XP_IP_USAGE_CITY_DB_PATH` / `XP_IP_USAGE_ASN_DB_PATH`。
- 不做需要凭据的第三方 Geo API 接入。

## 关键行为

- `geo_source` 默认为 `missing`；当 `XP_IP_GEO_ENABLED=true` 时为 `country_is`（启用后会把入站公网 IP 发送到第三方 `country.is` 做解析）。
- `XP_IP_GEO_ORIGIN` 可覆盖默认 `https://api.country.is`（用于自建同接口实现或特殊网络环境）。
- Geo 查询仅针对当前分钟新出现、且本地持久化记录尚无 Geo 的公网 IP；结果写入缓存后不重复查询。
- 节点主动探测链路会在启动后立即探测一次出口公网 IP，并周期性刷新；Geo 结果与订阅地区映射持久化后供 Mihomo 系统分组读取。
- 当节点主动探测失败时，保留最近一次成功 Geo/地区归类并标记 stale，不引入第二套人工 override 真相源。
- `country.is` 查询做本地节流，避免触发托管端限流；遇到 `429` 时按 `Retry-After`（或默认 60s）退避；其他失败退避 15 分钟。采集主流程继续运行。
- `online_stats_unavailable` warning 保留；`geo_db_missing` warning 删除。
- 旧 `SetGeoDbUpdateSettings` WAL 记录允许反序列化，但运行时按 no-op 处理。

## 验收

- 代码与 UI 中不再出现 `/ip-geo-db`、`XP_IP_USAGE_CITY_DB_PATH`、`XP_IP_USAGE_ASN_DB_PATH`。
- Node/User IP usage 仍可返回地区与运营商字段；当 `geo_source=country_is` 时展示 `country.is` attribution。
- 节点详情与订阅渲染可复用同一 `country.is` 结果驱动 `subscription_region`，且不依赖本地 MMDB 文件。
- `cargo test`、`cargo clippy -- -D warnings`、`cd web && bun run lint`、`cd web && bun run typecheck`、`cd web && bun run test` 通过。

## 变更记录

- 2026-03-10: 完成 `country.is` hosted IP Geo hard cut，移除本地 MMDB / DB-IP 依赖。
- 2026-04-24: 复用 `country.is` Geo 解析为节点主动探测出口 IP 提供订阅地区分类能力，并新增 stale 保留语义。
