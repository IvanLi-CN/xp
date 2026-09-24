# Release 失败 Telegram 告警历史

## Decision Trace

- 通知入口从仓库外部的旧共享 workflow 迁移到固定 commit SHA 的 Oidrune workflow。
- 由 xp caller 显式生成完整 summary，保留失败发布与手动 smoke 的项目语义。

## Compatibility

- `workflow_run` 的 release/main/failure 过滤、目标 SHA 解析与 `workflow_dispatch` smoke 触发方式
  保持不变。
- 旧 secret-based notifier wiring 不再属于 xp 的通知合同。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
