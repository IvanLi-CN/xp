# SS2022 接入点 Mihomo SMux 策略实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖与 rollout 相关事实。

## Current Status

- Implementation: complete
- Lifecycle: active

## Coverage / rollout summary

- SS2022 元数据保存 `mihomo_smux`；旧 SS2022 metadata 通过反序列化默认值获得同一策略。
  既有 VLESS metadata 继续可读以保持 state/API 兼容，但订阅渲染忽略它。
- 创建未提供策略时持久化默认值；PATCH 只替换完整策略，并拒绝 `null` 或越界数值。
- Clash 与 Mihomo system provider YAML 的直连、链式 SS2022 proxy 按接入点策略输出 SMux；
  VLESS Reality/Vision 永不输出该字段，Raw/Base64 URI 不变。
- 接入点新建和详情页仅为 SS2022 提供可编辑的高级设置；旧 API 未广告
  `admin.endpoint-mihomo-smux` 时界面不发送该字段。
- 脱敏 VLESS Reality 与 SS2022 fixture 已由官方 Mihomo `v1.19.29` 的 `-t` 配置检查解析通过。

## Validation Coverage

- Rust 覆盖默认反序列化、SS2022 创建和更新、非法范围拒绝、VLESS 无 SMux、YAML
  直连/链式条目与 Raw URI 不变。
- Web 覆盖 SS2022 新建默认策略、详情页旧数据回退、关闭后保留字段、数值范围与 API 负载，
  并确认 VLESS 不展示或发送 SMux 设置。
- Storybook 覆盖 SS2022 的默认 SMux 高级设置交互，并提供 mock-only 画布证据。

## References

- `./SPEC.md`
- `./HISTORY.md`
