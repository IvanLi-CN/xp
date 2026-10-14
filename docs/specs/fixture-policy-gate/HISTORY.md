# Fixture 数据策略门禁演进历史

> 这里记录影响长期合同的关键原因；规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 选择 `ast-grep` 作为唯一 fixture-policy scanner，并将其限定为静态源代码 gate。
- 选择单一 JSON catalog 加两种语言 adapter，避免测试源码持有操作型 fixture 字面量。
- 禁止受保护路径的 suppression，避免同一变更同时引入数据和禁用检测。

## Key Reasons / Replacements

- catalog 的值由规则独立校验，不能以编辑 catalog 的方式隐式批准任意输入。
- Rust adapter 作为 path dev crate 提供给 unit 与 integration tests，不进入生产构建。

## References

Web Demo 复用正式 mock 写入后，独立身份序列避免与五个种子用户冲突。工厂归属统一到 catalog，并同步精确规则白名单及拒绝反例。

- `./SPEC.md`
- `./IMPLEMENTATION.md`
