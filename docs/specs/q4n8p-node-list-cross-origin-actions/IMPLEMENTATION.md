# Implementation

## Frontend

- `NodeInventoryList` 现在以容器宽度 `768px` 作为唯一卡片降级阈值：桌面与中间宽度统一保持表格，移动端才切卡片。
- 共享节点列表新增 `Actions` 列与复用的 `NodeRowActions`，每行固定提供 `Details` 和 `Open on node` 两个按钮；非法或非 HTTPS 的 `api_base_url` 会被防御性禁用。
- 新增 `web/src/utils/navigation.ts`，统一承载跨节点 href 合成、`login_token` 覆盖与 `redirect` 路径清洗规则，供列表、路由守卫与登录页共用。
- `router.tsx` 的受保护路由会把未登录目标页写入 `redirect`，并把原始 `login_token` 从回跳 URL 中剥离后单独透传给 `/login`。
- `LoginPage` 自动消费 `login_token` 后会按校验后的相对 `redirect` 回跳，完整保留 query/hash；非法目标回退 `/`。

## Coverage

- 组件与页面单测更新为双按钮/跨节点跳转契约：`src/components/NodeInventoryList.test.tsx`、`src/views/NodesPage.test.tsx`、`src/views/HomePage.test.tsx`、`src/views/LoginPage.test.tsx`。
- 新增 `src/utils/navigation.test.ts` 覆盖 `redirect` 清洗、`login_token` 剥离与跨节点 href 合成。
- Storybook 更新 `NodeInventoryList` 与 `NodesPage` 场景，并通过 `test-storybook` 覆盖桌面表格、移动卡片和同页跳转 href 合成。

## Validation

- `cd web && bun run lint`
- `cd web && bun run typecheck`
- `cd web && bun run test`
- `cd web && bun run test-storybook -- --config-dir .storybook --url http://127.0.0.1:22080`
