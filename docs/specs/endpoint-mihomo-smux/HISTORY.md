# 接入点 Mihomo SMux 策略演进历史

> 这里记录影响长期契约的决策；规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 选择按接入点保存的策略，而非全局或按用户策略，确保同一接入点的所有 YAML 订阅入口一致。
- 选择默认启用并兼容旧元数据，避免现网接入点需要迁移后才获得连接复用。
- 选择只向 YAML 输出下发；VLESS/SS URI 无标准化 SMux 表达，不能伪造协议参数。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
