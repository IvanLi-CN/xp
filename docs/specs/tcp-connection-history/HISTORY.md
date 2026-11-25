# History

- 2026-11-25: 新增全命名空间连接历史合同。采样从入站监听端口扩大到所有 `ESTABLISHED` TCP
  socket，并按 `inbound`、`control_plane`、`outbound_runtime`、`outbound_other` 分类；
  total 是同刻四类之和。本地 raw 保持 7 天，新增 5m 与 90 天 daily rollup，并使用专用
  History Stream。
- 接续 `node-tcp-connection-count` 的入站范围：入站口径继续有效，出站与全站分类归本主题。
