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

## Validation Notes

- Unit, integration, Web checks, Impeccable detection, Storybook interaction, and local visual
  validation run on this topic branch.
- Read-only directed-edge checks found no signed `health-v2` acknowledgement from the current
  Reality endpoints; public fallback observations are kept separate from Mesh active success.
- Shared testbox fetched the real Xray suite but GHCR image resolution ended with EOF.
- The failed testbox run removed its isolated remote directory and Docker resources.

## References

- `./SPEC.md`
- `./HISTORY.md`
