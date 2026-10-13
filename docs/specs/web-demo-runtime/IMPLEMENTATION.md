# Web Demo Runtime 实现覆盖

## Current Status

- Implementation: complete for the local Demo runtime and Mihomo walkthrough
- Lifecycle: active

## Implementation Coverage

独立 Demo 运行时已替换依赖 localStorage 登录及复制页面的演示方案。正式页面、模拟传输层、结构化 Inspector、静态深链和浏览器验收共同覆盖合同。

## Remaining Gaps

公开部署不属于本轮范围。生产静态构建仍输出较大的单入口 bundle，这是现有 formal App 的打包形态，不影响 Demo 的启动隔离和静态深链恢复。

## Verification Evidence

- `bun run build:demo`
- `E2E_BASE_URL=http://127.0.0.1:22291 E2E_REUSE_EXISTING_SERVER=1 bunx --no-install playwright test tests/e2e/demo-user-details.spec.ts tests/e2e/demo-service-monitors.spec.ts --workers=2`
- `bun run build-storybook`
- `bun run lint`
- `bun run typecheck`
- `bun run check:style-budget`
- `git diff --check`

The browser suite covers direct formal user routes without a login gate, URL-first identity switching, Mihomo mirror preview, structured missing-profile errors, normal service monitoring, and incident state. The static preview was also opened directly at a nested `/demo/users/...` URL without a live API service.

## References

- `./SPEC.md`
- `./HISTORY.md`
