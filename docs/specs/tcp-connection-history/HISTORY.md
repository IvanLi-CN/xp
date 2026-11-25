# History

- 2026-11-25: 新增全命名空间连接历史合同。采样从入站监听端口扩大到所有 `ESTABLISHED` TCP
  socket，并按 `inbound`、`control_plane`、`outbound_runtime`、`outbound_other` 分类；
  total 是同一采样过程四类之和，`truncated` 或类别未观测时为下界。节点本地保留 1m/5m 7 天，
  Repository 只保留 90 天 daily，并使用专用 History Stream。
- 扩展并取代 `node-tcp-connection-count` 的端点求和展示语义；入站采样口径仍被引用。
- 保留旧 admin 响应字段与 `connections.v1` payload；混合版本按 target/repository capability
  降级。endpoint 删除用 Raft-backed registry 过滤 subject，历史 total/classes 不可变。
