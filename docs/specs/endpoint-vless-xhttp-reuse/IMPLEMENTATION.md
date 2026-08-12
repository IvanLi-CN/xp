# VLESS XHTTP/XMUX 单连接复用实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Implementation: complete
- Lifecycle: local verification complete
- Catalog note: XHTTP transport、XMUX subscription、管理界面与真实客户端复用验证均已完成。

## Coverage / rollout summary

- Metadata 采用 legacy `vision_tcp` / new `xhttp` 双默认边界。
- Xray SplitHTTP proto、dynamic inbound 与 transport-specific user flow 已接入。
- Mihomo YAML/URI 覆盖 Clash、direct、chain 与 system provider。
- API capability、创建/PATCH 和管理 Web advanced control 已纳入实现范围。
- 官方 Mihomo v1.19.29 已接受脱敏 XHTTP fixture；真实 Xray Reality + Mihomo E2E 已验证
  预热后 64 个并发代理请求共享一条外部 TCP，断链后自动以第二条连接恢复。

## Rollout Boundary

- 合并、发布、现网 transport 切换与客户端订阅刷新需要单独授权。
- 切换现有 endpoint 前，先确认客户端已升级到 Mihomo v1.19.29 或更高版本，并准备在
  inbound 重建后立即刷新 YAML 订阅。

## Related Changes

- None

## References

- `./SPEC.md`
- `./HISTORY.md`
