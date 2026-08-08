# 接入点 Mihomo SMux 策略实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖与 rollout 相关事实。

## Current Status

- Implementation: complete
- Lifecycle: active

## Coverage / rollout summary

- VLESS Reality 与 SS2022 元数据均保存 `mihomo_smux`；旧元数据通过反序列化默认值获得同一策略。
- 创建未提供策略时持久化默认值；PATCH 只替换完整策略，并拒绝 `null` 或越界数值。
- Clash 与 Mihomo system provider YAML 的直连、链式 VLESS/SS2022 proxy 均按接入点策略输出 SMux；Raw/Base64 URI 不变。
- 接入点新建和详情页均提供可编辑的高级设置，并在关闭后保留管理员已填数值。
- 脱敏 VLESS Reality 与 SS2022 fixture 已由官方 Mihomo `v1.19.29` 的 `-t` 配置检查解析通过。

## Validation Coverage

- Rust 覆盖默认反序列化、VLESS/SS2022 创建和更新、非法范围拒绝、YAML 直连/链式条目与 Raw URI 不变。
- Web 覆盖新建默认策略、详情页旧数据回退、关闭后保留字段、数值范围与 API 负载。
- Storybook 覆盖新建与详情页的默认 SMux 高级设置交互，并提供 mock-only 画布证据。

## References

- `./SPEC.md`
- `./HISTORY.md`
