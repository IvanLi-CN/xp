# History

- 2026-07-29: release builds use Fat LTO, size optimization, abort-on-panic,
  one codegen unit, and stripped symbols after production PSS showed that
  cloudflared's live heap was already small and mapped executable pages were
  the remaining controllable budget.

- 2026-07-29: release builds use Thin LTO, one codegen unit, and stripped
  symbols to reduce XP's mapped code footprint; OpenRC backfill now supports
  provider wrapper scripts without `command_user`.

- 2026-07-29: OpenRC memory backfill now preserves executable service-script
  permissions after atomic replacement.

- 2026-07-29: upgrade backfill now reloads systemd and restarts cloudflared so
  runtime defaults are active before an upgrade is reported successful.

- 2026-07-29: added low-memory administrator authentication, child-process Go
  heap defaults, upgrade backfill, and PSS sampling.

- 2026-07-29: created after production OOM diagnosis on a 128 MB node.
- The 64 MiB aggregate PSS target supersedes the memory sizing assumption in #r7m2q without
  replacing its restart/backoff contract.
- The 4 MiB Argon2 profile supersedes the historical 64 MiB profile in #38wmj while preserving
  the PHC storage model.
