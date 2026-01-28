# CLI contract: `xp-ops upgrade`

## Summary

`xp-ops upgrade` is the single recommended upgrade entrypoint. It upgrades both:

- `xp` (installed at `/usr/local/bin/xp`)
- `xp-ops` (the currently running executable)

Both upgrades resolve the same GitHub Release (default: latest stable).

## Command

```
xp-ops upgrade \
  [--version <SEMVER|latest>] \
  [--prerelease] \
  [--repo <owner/repo>] \
  [--dry-run]
```

## Semantics

- `--version` defaults to `latest`.
- `--prerelease` is only valid when `--version latest` is used; it selects the newest prerelease by `published_at`.
- `--repo` overrides the default source repo; it is equivalent to `XP_OPS_GITHUB_REPO`.
- `--dry-run` prints resolved release + planned actions without downloading/writing/restarting.
