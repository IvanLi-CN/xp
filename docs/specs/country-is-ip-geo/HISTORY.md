# History

- The topic originated in legacy Spec `#tc2kp`.
- The hosted IP-geo contract superseded the prior local database maintenance path.

## 变更记录

- 2026-03-10: 完成 `country.is` hosted IP Geo hard cut，移除本地 MMDB / DB-IP 依赖。
- 2026-04-24: 复用 `country.is` Geo 解析为节点主动探测出口 IP 提供订阅地区分类能力，并新增 stale 保留语义。
