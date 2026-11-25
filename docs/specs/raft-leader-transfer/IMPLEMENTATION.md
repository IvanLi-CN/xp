# Raft leader election handoff implementation

> Current behavior is defined by `./SPEC.md`.

## Current Status

- Implementation: implemented for the local signed route and `xp-ops` command.
- Lifecycle: active.
- Catalog note: requires the target to run this build or newer.

## Implementation Coverage

- `RaftFacade::elect` triggers a local OpenRaft election and waits for local leader metrics with a
  30-second bound.
- The signed internal route validates the sender identity, local voter membership, non-joint
  membership, no active membership operation, and applied log progress before election.
- `xp-ops xp transfer-leader` uses the target's local cluster CA identity and the signed internal
  route; it does not read an administrator token.
- The operator-facing command and failure boundaries are documented in `docs/ops/README.md`.

## Verification

- `cargo fmt --all -- --check`: passed
- `cargo test --lib`: 2127 passed, 4 ignored
- `cargo clippy -- -D warnings`: passed
- manual signed-route invocation on a disposable multi-voter cluster is required before production
  use

## Remaining Gaps

- No integration test against a live multi-voter Raft cluster is recorded in this change; routing
  and CLI compilation are covered by the full Rust suite.
- Older targets without the route cannot be used; upgrade the target first.

## Related Changes

- None.

## References

- `./SPEC.md`
- `./HISTORY.md`
