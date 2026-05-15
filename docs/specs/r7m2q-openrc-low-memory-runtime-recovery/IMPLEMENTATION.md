# Implementation

## Runtime changes

- `Config` defaults favor low-memory OpenRC hosts: xray probes every 5 seconds, marks down after 4 failures, and gives init-system restarts 20 seconds to complete.
- Runtime supervisors keep the first restart fast, then apply exponential backoff with a 300 second cap while a component remains down.
- OpenRC restarters audit same-service `supervise-daemon` and worker processes before and after restart, warn on duplicates, and terminate only `supervise-daemon <service>` PIDs that are not the pidfile-backed active supervisor; `xp-ops` installs and authorizes a narrow `xp-openrc-kill-supervisor` helper for root-owned supervisor cleanup.
- cloudflared monitoring is controlled separately from active restarts so operators can observe Tunnel state without letting `xp` repeatedly call `rc-service cloudflared restart`.
- Legacy configs that only set `XP_CLOUDFLARED_RESTART_MODE=openrc|systemd` still enable cloudflared monitoring through the effective monitor mode fallback; explicit `XP_CLOUDFLARED_MONITOR_MODE=none` and legacy `XP_CLOUDFLARED_RESTART_MODE=none` opt-outs remain disabled.

## Health output

- `/api/health` remains additive and backward-compatible.
- `xray` and `cloudflared` blocks include restart attempts, last restart timestamps, next restart time, backoff counters, and whether automatic restart is enabled.

## Rollout notes

- Existing nodes keep explicit env overrides until operators rewrite `/etc/xp/xp.env`.
- New host-managed deployments should receive `XP_CLOUDFLARED_MONITOR_MODE=<init-system>` and `XP_CLOUDFLARED_RESTART_MODE=none` from env backfill.
