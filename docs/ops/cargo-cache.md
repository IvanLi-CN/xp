# Cargo cache contract

This contract applies to the Mesh resource runner. It is independent of the
shared-testbox skill: the runner receives explicit cache paths and does not
derive them from a host name or an Agent identifier.

## Cache ownership

`CARGO_HOME` may be shared by compatible Linux agents for Cargo's registry and
Git dependency downloads:

```text
/srv/codex/caches/linux-amd64/cargo/
```

Compiled project output stays private to the current Agent Directory. For a
shared-testbox task, select a cache root inside the directory returned by
`testbox-agent-dir.sh`, for example:

```text
/srv/codex/agents/<CODEX_THREAD_ID>/xp-cargo-cache/
  source/candidate/
  source/baseline/
  target/<rustc-fingerprint>/candidate/
  target/<rustc-fingerprint>/baseline/
```

The candidate and baseline target directories must remain separate. Cargo's
fingerprints decide whether a unit is reusable after the matching source slot,
Web assets, toolchain and build environment are restored. A target directory is
never shared between Agents and is removed only with the owning Agent Directory.

## Runner usage

Resource runs require both explicit paths:

```bash
XP_TESTBOX_CARGO_CACHE_ROOT=/srv/codex/agents/$CODEX_THREAD_ID/xp-cargo-cache \
XP_TESTBOX_CARGO_HOME=/srv/codex/caches/linux-amd64/cargo \
XP_RUN_MESH_RESOURCE=1 \
scripts/testbox/run-shared-quota-xray-e2e.sh
```

The runner synchronizes immutable candidate and baseline source snapshots into
stable cache slots, builds through `scripts/cargo-cache/with-cargo-target.sh`,
and copies the resolved XP and resource-test executables into the disposable
run before measurement. The run directory is cleaned by the existing runner
trap; it is not a cache.

Use `scripts/cargo-cache/status.sh --cache-root PATH` to inspect both slots.
The status command is read-only. `scripts/cargo-cache/test.sh` is the local
fixture for the wrapper's Cargo invalidation, path, slot and lock behavior.
`scripts/testbox/test-shared-quota-xray-e2e-guards.sh` exercises the runner's
early cache and formal/smoke gate guards without connecting to testbox.

## Gate rules

The formal candidate-versus-baseline resource gate remains 900 seconds per
side, with the existing 128 MiB/no-swap, HTTP/2, PSS and CPU assertions. Set
`XP_TESTBOX_CACHE_SMOKE=1 XP_MESH_RESOURCE_DURATION_SECS=600` only for a
non-acceptance cache smoke; its evidence cannot satisfy the formal gate.

After evidence is collected and no further reuse is needed, clean the exact
Agent Directory with the existing `testbox-clean.sh --apply` helper. Do not
delete the shared Cargo download cache from a project runner.
