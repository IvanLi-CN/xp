# 发布意图治理演进历史

> 这里记录会影响 Agent 理解“为什么一步步变成现在这样”的关键演进。

## Decision Trace

- 采用 GitHub PR Issue events 截断到 merged event 的重放结果作为正常发布意图。
- 采用显式 Manual Backfill 输入修复历史发布，不修改历史 PR 标签，不创建自有记录。
- 发布 readiness 从单一 `ci` 扩展为目标 SHA 的 `ci`、`fixture-policy`、`xray-e2e` 三条 main push workflow。

## Key Reasons / Replacements

- 当前 PR labels 会在合并后变化，不能作为历史发布决定的稳定来源。
- #267 曾因 `type:skip` 被跳过，纠正发布必须锁定其合并提交与 `3.33.0` expected version。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
