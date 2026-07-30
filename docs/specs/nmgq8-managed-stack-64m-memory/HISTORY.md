# History

- OpenRC restart commands can return before `supervise-daemon` reports its child started.
  Host upgrades now wait for manager readiness and roll back on timeout.

- Host-managed upgrades now complete the locked XP and managed runtime phase before
  replacing `xp-ops`, preventing a successful tool self-update from skipping the
  corresponding service release.

- Joined Docker/Compose nodes now reconcile an explicitly configured low-memory
  administrator PHC into the persisted cluster state at startup. This keeps
  token rotation on the host Compose path and rejects high-memory PHCs before
  any running XP process accepts them.

- 2026-07-29: a provider network blocked cloudflared's outbound QUIC/7844 on
  HK2. The tunnel connected with HTTP/2 and served the public login normally,
  so all managed launch paths now default to `--protocol http2`, retain an
  explicit `XP_CLOUDFLARED_PROTOCOL=quic` operator override, and reload/restart
  a changed managed service definition before reporting success.

- 2026-07-29: a production SG canary showed that Go executable mappings were a
  material part of the remaining PSS. Release automation now builds pinned
  Xray and cloudflared sources with inlining disabled, publishes checksummed
  assets for both Linux architectures, and installs them as a rollback unit.

- 2026-07-29: cloudflared moved to `GOMEMLIMIT=8MiB` with management diagnostics
  disabled after the combined canary reached a `63,632 KiB` 60-second peak.

- 2026-07-29: text-based embedded Web assets are stored as deterministic gzip
  payloads with HTTP content negotiation, reducing release executable mappings
  while preserving uncompressed-client compatibility.

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
