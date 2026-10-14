# Web Demo Runtime

## Context and Scope

XP Web Demo 提供可直接体验、可分享复现的正式管理界面。演示运行时与真实运行时复用页面、路由后缀、组件及 API 调用合同，通过安全的内存模拟后端提供稳定输入。

In scope: 独立 Demo 构建、本地演示、启动隔离、正式页面复用、Inspector、分享状态、模拟写入与深链恢复。Out of scope: 公开部署、真实后端权限变更及真实升级、集群或基础设施操作。

## Terms and Interfaces

- Demo Runtime: 只连接内存模拟后端的产品演示运行时。
- Demo Scene: 可确定性重建的数据场景。
- Demo Persona: 模拟的认证身份，遵循正式产品权限模型。
- Demo Inspector: 演示场景、身份、网络、数据和动作的结构化控制面板。
- `build:demo` produces a standalone static application rooted at `/demo/`.
- `demo` selects a scene; `d_*` parameters encode shareable controls independently of product parameters.

## Requirements

### REQ-DEMO-001

Demo MUST directly open documented deep links with a default authenticated synthetic administrator and normal scene. No stored session or prior visit to a login page is required. Login is an explicit scenario, not the default entry gate.

### REQ-DEMO-002

Demo MUST reuse the live route tree, page components and API call surface. Only the runtime, data transport and Inspector are Demo-specific. Unsupported simulated actions MUST report a mock error without calling a real backend.

### REQ-DEMO-003

The mock transport MUST be installed before rendering, authentication bootstrap, data requests or stream subscriptions. Failed bootstrap MUST prevent App rendering. Demo MUST neither read nor overwrite real authentication, backend profiles or persisted API query state, and MUST never fall back to live API traffic.

### REQ-DEMO-004

Inspector MUST expose structured scene, persona, network, data, actions and share controls. Wide-screen docking MUST preserve the normal App layout, desktop floating controls MUST remain within the viewport, and mobile MUST use a recoverable bubble and drawer. Every layout MUST allow collapse and restoration.

### REQ-DEMO-005

Scene, persona, network and predefined data controls MUST be URL-first, survive refresh and sharing, and update through client-side navigation without document reload. Panel visibility and placement MUST remain local-only. Simulated writes MUST remain in memory and reset on reload or explicit reset; share URLs MUST not contain entered secrets or arbitrary mutation payloads.

Subscription preview MUST follow these history rules:

- Back and Forward restore visibility from `d_preview`, including already cached previews.
- Opening preview adds one entry; its format and mirror changes replace that entry.
- Close returns to the immediately preceding opener when that entry is identifiable.
- Otherwise, close removes `d_preview` in place and stays on the same user page.

### REQ-DEMO-006

Simulated writes MUST update the formal UI and a bounded visible operation journal. Journal entries MUST record only method, fixed route identity and result, never credentials or request bodies. Demo personas MUST reflect XP's administrator/unauthenticated model rather than invent production roles.

### REQ-DEMO-007

The standalone Demo build MUST use `/demo/` as its router and asset base, keep the live build unchanged, and emit `404.html` plus `_redirects` for static deep-link recovery. A supported static host MUST apply that fallback for `/demo/*`. Root, nested direct navigation and nested refresh MUST work independently of a real API service.

Correctly served Demo pages MUST preserve live and unrelated Service Worker registrations.

Legacy recovery requires a controlling worker serving the live XP shell at a Demo URL.

Only that controlling registration may be unregistered during legacy recovery.

### REQ-DEMO-008

The Mihomo Mirror walkthrough MUST use the formal user detail controls. Original mode remains the default. Copy URL, fetch and preview MUST agree on the selected mode. Valid HTTPS resource profiles MUST show rewritten resource URLs; missing or invalid profiles MUST return the structured error contract without presenting fabricated successful YAML. Raw and Clash MUST not show the Mirror control.

## Verification

### VER-DEMO-001

- Method: runtime unit tests and browser integration tests.
- covers: `REQ-DEMO-001`, `REQ-DEMO-002`, `REQ-DEMO-003`, `REQ-DEMO-006`
- Pass condition: fresh deep links open formal pages, simulation mutations are visible, and all API requests remain intercepted including unknown routes and faults.

### VER-DEMO-002

- Method: desktop/mobile Inspector browser tests and complete-page visual evidence.
- covers: `REQ-DEMO-004`, `REQ-DEMO-005`
- Pass condition: controls synchronize URL and runtime, sharing/refresh reproduce the scene, layout can collapse and recover without overflow or reload.

### VER-DEMO-003

- Method: standalone static-build smoke with no backend and Mihomo walkthrough tests.
- covers: `REQ-DEMO-007`, `REQ-DEMO-008`
- Pass condition: static nested routes recover on direct load and refresh, and subscription success/error/copy states match the selected controls.

## Related ADRs

None

## Visual Evidence

- source_type=ui_demo · target_program=mock-only · capture_scope=page · requested_viewport=1280x900
  - state: `/demo/users/user-fixture-c?demo=normal&d_profile=valid&d_format=mihomo&d_mirror=mirror&d_preview=preview`
  - evidence_note: Formal user details page, Mihomo mirror preview, rewritten resource URLs and recoverable Inspector.
  - ![Web Demo desktop](../mihomo-external-resource-mirror/assets/mihomo-resource-mirror-demo-desktop.png)
- source_type=ui_demo · target_program=mock-only · capture_scope=page · requested_viewport=393x852
  - state: `/demo/users/user-fixture-c?demo=normal&d_profile=valid&d_format=mihomo&d_mirror=mirror&d_preview=preview`
  - evidence_note: Mobile subscription preview and bottom-sheet Inspector remain usable without login or reload.
  - ![Web Demo mobile](../mihomo-external-resource-mirror/assets/mihomo-resource-mirror-demo-mobile.png)

## References

- `./IMPLEMENTATION.md`
- `./HISTORY.md`
