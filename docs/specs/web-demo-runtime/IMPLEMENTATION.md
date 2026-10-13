# Web Demo Runtime 实现覆盖

## Current Status

- Implementation: complete for the local Demo runtime and Mihomo walkthrough
- Lifecycle: active

## Implementation Coverage

Demo runtime uses formal pages, mocks, Inspector, and static deep links.

## Remaining Gaps

Public deployment is out of scope; local evidence covers static deep links.

## Verification Evidence

- `bun run build:demo`
- `bun run test:e2e:demo`
- `bun run test --run src/demo`
- Static Demo E2E command with preview build
- `bun run build-storybook`
- `bun run lint`
- `bun run typecheck`
- `bun run check:style-budget`
- `git diff --check`

Demo E2E covers routes, identity, mobile recovery, Mihomo errors, and monitoring.

Static E2E serves dist-demo fallback files and checks nested refresh offline.

## References

- `./SPEC.md`
- `./HISTORY.md`
