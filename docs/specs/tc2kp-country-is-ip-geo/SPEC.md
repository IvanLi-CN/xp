# country.is Hosted IP Geo hard cut

## 背景

原先的入站 IP Geo 依赖本地 MMDB 文件与 DB-IP Lite 下载更新。该方案会占用本地磁盘，并在更新失败时留下临时文件，已经不适合作为默认方案。

## 目标

- 用免费、无鉴权、零门槛的 `country.is` Hosted API 替换本地 MMDB 方案。
- 删除 Geo DB 设置页、手动更新、托管下载与外部 MMDB override 能力。
- 保持 Node/User IP usage 继续输出 `country / region / city / operator`，并在 UI 中标注 `country.is` attribution。

## 非目标

- 不保留 `/ip-geo-db` 只读页或兼容入口。
- 不继续支持 `XP_IP_USAGE_CITY_DB_PATH` / `XP_IP_USAGE_ASN_DB_PATH`。
- 不做需要凭据的第三方 Geo API 接入。

## 关键行为

- IP Geo 由前端 `Cluster settings` 统一控制；保存后通过 Raft 复制到集群状态，不再要求逐节点修改 env。
- 在首次保存 `Cluster settings` 之前，leader 当前进程仍使用 legacy fallback `XP_IP_GEO_ENABLED` / `XP_IP_GEO_ORIGIN` 作为 bootstrap seed。
- `geo_source` 默认为 `missing`；当集群级 `ip_geo.enabled=true` 时为 `country_is`（启用后会把入站公网 IP 发送到第三方 `country.is` 做解析）。
- 集群级 `ip_geo.origin` 默认 `https://api.country.is`，可覆盖为自建同接口实现或特殊网络环境的 `http(s)` 绝对 URL。
- Geo 查询仅针对当前分钟新出现、且本地持久化记录尚无 Geo 的公网 IP；结果写入缓存后不重复查询。
- `country.is` 查询做本地节流，避免触发托管端限流；遇到 `429` 时按 `Retry-After`（或默认 60s）退避；其他失败退避 15 分钟。采集主流程继续运行。
- `online_stats_unavailable` warning 保留；`geo_db_missing` warning 删除。
- 复用旧 `SetGeoDbUpdateSettings` 作为兼容传输载体：当 payload 的 `provider` 是 `http(s)` URL 时按新的集群 IP Geo 设置处理；历史非 URL payload 继续按 no-op 处理。

## 验收

- 代码与 UI 中不再出现 `/ip-geo-db`、`XP_IP_USAGE_CITY_DB_PATH`、`XP_IP_USAGE_ASN_DB_PATH`，且前端提供独立的 `Cluster settings` 页面作为 IP Geo 主入口。
- Node/User IP usage 仍可返回地区与运营商字段；当 `geo_source=country_is` 时展示 `country.is` attribution。
- `cargo test`、`cargo clippy -- -D warnings`、`cd web && bun run lint`、`cd web && bun run typecheck`、`cd web && bun run test` 通过。
