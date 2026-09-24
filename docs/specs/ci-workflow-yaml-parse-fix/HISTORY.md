# History

- The topic originated in legacy Spec `#5yd72`.
- It records a completed, one-time repair to the release workflow syntax.

## 变更记录（Change log）

- 2026-03-05: 创建规格，冻结“仅修复 workflow 语法并恢复发版链路”的范围。
- 2026-03-05: 完成 `ci.yml` 语法修复与本地 YAML 解析验证（M1/M2）。
- 2026-03-05: PR #98 checks 全绿（`ci`/`pr-label-gate`/`xray-e2e`），并补齐 CI 合约断言鲁棒性与既有 clippy lint 修复，完成 M3。
