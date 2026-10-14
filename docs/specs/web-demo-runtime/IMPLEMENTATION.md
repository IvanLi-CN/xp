# Web Demo Runtime 实现覆盖

## Current Status

- Implementation: complete for the local Demo runtime and Mihomo walkthrough
- Lifecycle: active

## Implementation Coverage

Demo runtime uses formal pages, mocks, Inspector, and static deep links.

Preview follows URL history. Close returns to its opener; direct links close in place.

Brand assets use the build base. Correct Demo pages preserve live workers.

Catalog factories own Demo user IDs and tokens; each model has an isolated sequence.

Fixture rules accept only the exact catalog-derived Demo factory and reject external namespaces.

Vitest resolves the same live Demo frame entry as the production build.

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
