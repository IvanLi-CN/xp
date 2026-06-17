# Real-Xray E2E runners need serial tests and hardened shared-testbox preflight

## Symptoms

- GitHub Actions `xray-e2e` passes `tests/xray_e2e` but then fails in `tests/shared_quota_xray_e2e` with `Broken pipe` during the large SS2022 roundtrip.
- The same ignored suites can pass on another machine when run one after another under a stricter harness.
- Shared testbox compose runs can also fail before the tests start with Docker reporting `all predefined address pools have been fully subnetted`.
- After OpenSSL vendoring lands, a remote Linux testbox can spend minutes compiling and then fail because `make` is missing on the host.

## Root cause

Two separate guardrails were missing from the real-Xray runners:

1. The ignored external-Xray suites share one live Xray daemon plus one forwarded SS port. If libtest is allowed to run those ignored tests concurrently inside the same binary, they can race on inbound state and socket usage.
2. The shared testbox runner relied on Docker's default address-pool allocation and did not preflight the host toolchain needed by vendored OpenSSL builds.

That combination makes the real-Xray harness look flaky even though the product behavior is unchanged.

## Fix used here

- Force `RUST_TEST_THREADS=1` in `scripts/e2e/run-local-xray-e2e.sh` before running the ignored real-Xray suites.
- Keep the shared testbox runner on one isolated compose run, but generate a per-run override that pins the default Docker network to a free `10.203.x.0/24` subnet.
- Fail fast on the shared testbox if `make` is missing, because vendored `openssl-sys` now needs it during remote `cargo test`.

## Guardrails / Reuse notes

- Any future real-Xray helper that reuses one external Xray process across multiple ignored test cases should pin test threads explicitly instead of relying on libtest defaults.
- Shared Docker hosts with many compose projects should not depend on the daemon's predefined address pools remaining available forever; pick an explicit per-run subnet from an agreed private range.
- When the repo depends on vendored OpenSSL, shared Linux runners need the minimal host build toolchain (`cc`, `ar`, `ranlib`, `perl`, `make`) even if the project itself is otherwise pure Rust.
- If the shared testbox is reprovisioned, repair it through the shared-testbox bootstrap path before blaming the Xray tests.

## References

- `docs/specs/t8m4q-xray-stale-tcp-reclaim/SPEC.md`
- `docs/solutions/ci/musl-release-openssl-vendoring.md`
- `scripts/e2e/run-local-xray-e2e.sh`
- `scripts/testbox/run-shared-quota-xray-e2e.sh`
