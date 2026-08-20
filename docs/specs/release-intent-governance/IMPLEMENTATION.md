# 发布意图治理实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Implementation: in progress
- Lifecycle: active
- Catalog note: resolver and readiness implementation is under review

## Coverage / rollout summary

- `release_intent.py` replays PR events through the unique merge event and validates type/channel.
- `release_readiness.py` evaluates exact-SHA main push workflow runs.
- `workflow_dispatch` requires explicit release type, channel, expected version, and reason.

## Remaining Gaps

- Governance PR convergence and the `v3.33.0` corrective release remain pending.
- Live GitHub Release and GHCR evidence is produced only after the governance PR is merged.

## Related Changes

- `.github/workflows/release.yml`
- `.github/workflows/label-gate.yml`
- `.github/scripts/release_intent.py`
- `.github/scripts/release_readiness.py`

## References

- `./SPEC.md`
- `./HISTORY.md`
