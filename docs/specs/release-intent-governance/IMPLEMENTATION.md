# 发布意图治理实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout
> 相关事实。

## Current Status

- Implementation: complete
- Lifecycle: active
- Catalog note: governance is merged and the corrective `v3.33.0` release is verified

## Coverage / rollout summary

- `release_intent.py` replays PR events through the unique merge event and validates type/channel.
- `release_readiness.py` evaluates exact-SHA main push workflow runs and reports all run
  diagnostics.
- `workflow_dispatch` requires explicit release type, channel, expected version, and reason.
- PR label gating reads the current label API, while automatic intent remains merge-event-only.

## Delivery Evidence

- Governance PR #268 merged as `a633e02bac6ea6fe3098a349b76e46d9f9d102e2`; its automatic
  release run ended with `type:skip` and no artifacts.
- Target SHA `bd1defe4f211273b5ba925bfcf6673795583c484` passed `ci`, `fixture-policy`, and
  `xray-e2e` main push readiness; manual run `32342456887` completed successfully.
- Tag `v3.33.0`, GitHub Release assets/checksums, and GHCR `v3.33.0`, `3.33.0`, and `latest`
  multi-architecture manifests were verified.

## Operational Boundary

- When the release target changes a workflow file, the Actions `GITHUB_TOKEN` cannot push that
  tag without `workflow` scope. An owner must create the exact annotated tag with an authorized
  credential; the manual workflow then reuses it idempotently before building artifacts.

## Related Changes

- `.github/workflows/release.yml`
- `.github/workflows/label-gate.yml`
- `.github/scripts/release_intent.py`
- `.github/scripts/release_readiness.py`

## References

- `./SPEC.md`
- `./HISTORY.md`
