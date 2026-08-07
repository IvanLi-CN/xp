# Reality fallback 控制面 Mesh 与系统状态页实现状态（#56dtr）

> 有效行为以 `./SPEC.md` 为准。

## Current Status

- Implementation: complete pending final review.
- Lifecycle: active.
- Catalog: supersedes `nbs5f`.

## Delivered

- internal-auth v2、purpose-separated ack（完整 canonical request digest）与 strict bodyless
  canary ingress。
- per-peer HTTPS Mesh transport、breaker、fallback 与本地 telemetry；Raft、leader forwarding
  和 node-history remote traffic 共用同一条受控客户端路径；空白 `access_host` 会直接选择公网
  fallback，不会构造无效 Mesh URL。
- durable local internal idempotency ledger。
- Mesh status API、status SSE revision 与 System Status Web surface。
- Additive Mesh capability/reason fields with backward-compatible telemetry decoding;
  System Status row actions use shared 32x32 icon targets with consistent focus and tooltip
  behavior, while mobile text actions remain available.
- systemd、OpenRC、container cutover guard、可取消的 pre-consumption marker 与 operator documentation。
- Storybook state gallery、mock-only `ui_demo`、desktop/mobile visual evidence。
- Peer transport uses a constrained desktop grid for its data and action columns, while narrower
  viewports switch to full-width stacked rows. Storybook asserts the two desktop action targets are
  `32x32` and remain inside the row boundary.
- The mock-only System Status `ui_demo` covers presentation states but does not establish shared
  AppShell geometry. The real `/system-status` route has a Playwright content-boundary regression
  check for peer-row actions at the production screenshot viewport.
- Canary Mesh forwarding rebuilds the fixed XP loopback URL from the authenticated request's raw
  path and query. This keeps HTTP/2 absolute-form URIs from being appended as a second origin and
  preserves the URI bytes covered by internal-auth v2; requests that the URL client would normalize
  are rejected before loopback forwarding.

## Validation Notes

- Unit, integration, Web checks, Impeccable detection, Storybook interaction, and local visual
  validation run on this topic branch.
- Read-only directed-edge checks found no signed `health-v2` acknowledgement from the current
  Reality endpoints before the HTTP/2 loopback forwarding fix; public fallback observations are
  kept separate from Mesh active success.
- A real TLS regression sends the same signed `health-v2` request over HTTP/1.1 and HTTP/2,
  verifies the loopback path/query and validates the returned acknowledgement.
- Shared testbox fetched the real Xray suite but GHCR image resolution ended with EOF.
- The failed testbox run removed its isolated remote directory and Docker resources.

## References

- `./SPEC.md`
- `./HISTORY.md`
