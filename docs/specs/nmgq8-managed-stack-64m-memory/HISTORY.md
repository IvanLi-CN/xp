# History

- 2026-07-29: upgrade backfill now reloads systemd and restarts cloudflared so
  runtime defaults are active before an upgrade is reported successful.

- 2026-07-29: added low-memory administrator authentication, child-process Go
  heap defaults, upgrade backfill, and PSS sampling.

- 2026-07-29: created after production OOM diagnosis on a 128 MB node.
- The 64 MiB aggregate PSS target supersedes the memory sizing assumption in #r7m2q without
  replacing its restart/backoff contract.
- The 4 MiB Argon2 profile supersedes the historical 64 MiB profile in #38wmj while preserving
  the PHC storage model.
