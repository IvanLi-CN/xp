# VLESS XHTTP/XMUX 单连接复用演进历史

> 这里记录影响长期理解的关键演进；规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 观察到 Vision/TCP 在低请求率下仍维持多条 TCP；`connIdle` 仅负责回收，不提供复用。
- 顶层 Mihomo `smux` 曾被误认为可直接用于 VLESS，但官方实现表明它使用 sing-mux，无法由
  Xray VLESS Reality/Vision inbound 消费。
- 真实 Mihomo/Xray 对比验证显示，Vision/TCP 的并发流各占连接；gRPC 与 XHTTP 在 pool
  预热后均可把并发流复用到一条 HTTP/2 TCP。
- Xray 已将 gRPC transport 标记为 deprecated，因此选择 XHTTP/XMUX 作为长期模式。
- 同端口 Reality ALPN fallback 无法可靠分流 Vision/TCP 与 HTTP/2；最终采用单 endpoint
  单 transport、显式重建和订阅刷新的迁移边界。

## Key Reasons / Replacements

- XHTTP/XMUX 替代“给 VLESS 下发 SMux”的错误方案。
- 历史字段缺失保持 Vision/TCP，避免服务升级本身造成协议 hard cut。
- 固定一条连接、以负 keepalive period 关闭 H2 PING、无限复用次数，减少 socket、TLS
  建连与定时器资源。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
- `../endpoint-mihomo-smux/SPEC.md`
