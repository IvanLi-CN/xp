# Mihomo Subscription Node Import 实现状态

> 规范以 `./SPEC.md` 为准；本文件记录实现、交付和 rollout 事实。

## Current Status

- Implementation: 已实现，核心代码起始于提交 `cca3c1455960cbd93b8bda22fba493c012865756`，最终收口提交为 `51c5f544e797dcbf12f77e319b12a12538501647`；本轮补强了 YAML 预检、URI 边界、公共 URL 加载器和兼容性状态。
- Lifecycle: active
- Compatibility: 仅 3.22 声明导入能力；3.21/3.20 隐藏入口但保留 Profile 读写。

## Implementation Coverage

- Requirement coverage: Backend detects URL, Mihomo YAML, Base64, and URI input; it supports six protocols, bounds, naming, deduplication, and safe reports.
- Requirement coverage: The authenticated no-store API returns only a new-node fragment; the Web dialog changes the draft only after explicit Add, while Save persists it.
- Verification commands: `cargo test` passed with 1711 library tests and all executed integration tests passing; `cargo fmt --all -- --check`, `cargo clippy -- -D warnings`, `bun run check:style-budget`, `git diff --check`, Spec drift, and the visual-evidence document check passed. Web validation passed with 112 Vitest files and 536 tests, lint/typecheck/build, and the target Storybook interaction suite passed with 11 tests.
- Rollout facts: Sources, decoded content, reports, and history are not persisted; source/body are 2 MiB, candidates 512, and remote fetches 15 seconds/10 redirects. The bounded iterative YAML preflight rejects aliases/references, excessive structural depth or property chains, unsupported line endings, and unsafe flow/document-end drafts before `serde_yaml` expansion; scalar and block-scalar contexts are tracked so marker-like text remains valid data.
- Review facts: Final read-only API/security, Rust data-flow, and Web state/visual review surfaces reported no P0-P2 findings. The Rust review also exercised 360 combinations of sequence shape, scalar style, line ending, and marker context.
- Empirical acceptance: Storybook desktop and Storybook-native 393x852 mobile preview reports were observed; Add closes the Dialog and marks the draft Unsaved, while Save remains an explicit later action. The observed request sequence is preview POST, local Add only, then the existing PUT only after explicit Save.

## Coverage / rollout summary

- The boundary is a user-scoped preview, explicit local draft merge, and the existing Profile save operation.

## Remaining Gaps

- Fuzzing, shared-testbox resource-pressure evidence, and the operator-run 128 MiB/no-swap release-candidate gate were not part of this local implementation run.
- Owner approval is still required before treating the captured images as submitted PR evidence; no PR, merge, push, or release is authorized by this plan.

## Related Changes

- `cca3c1455960cbd93b8bda22fba493c012865756`: parser, API, Web dialog, tests, and 3.22 capability.
- `51c5f544e797dcbf12f77e319b12a12538501647`: hardening, final quality gates, and signed-off implementation closeout.
- The 3.22 compatibility profile keeps the historical `v3.22.5` source commit, uses an explicit `contractSourceCommit` overlay for the additive import contract, and records a separate historical `serverApiRoutes` snapshot so post-3.22 routes are not presented as supported by the old server; 3.21/3.20 omit import support.

## References

- `./SPEC.md`
- `./HISTORY.md`
