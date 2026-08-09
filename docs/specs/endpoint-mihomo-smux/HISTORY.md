# 接入点 Mihomo SMux 策略演进历史

> 这里记录影响长期契约的决策；规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 选择按接入点保存的策略，而非全局或按用户策略，确保同一接入点的所有 YAML 订阅入口一致。
- 将策略范围限定为 SS2022：官方 Mihomo `v1.19.29` 的 VLESS adapter 不接受 `smux`，因此
  VLESS Reality/Vision 的历史元数据只能为 API/state 兼容保留，不能进入 YAML。
- 选择为旧 SS2022 metadata 默认启用，并仅向 YAML 输出下发；VLESS/SS URI 无标准化 SMux
  表达，不能伪造协议参数。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
