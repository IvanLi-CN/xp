# Mesh Resource Failure Evidence

- Candidate commit: `5607923d807b79598c7b2b27e42c6d392476ac0b`
- Baseline commit: `ed109323103627f98363705f43f610c6fc093e61`
- Testbox run: `20261002_115625_5607923d807b_f98cc47b`
- Status: failed; this run cannot satisfy empirical acceptance.
- Environment: release XP and Xray, Linux `MemoryMax=128MiB`, `MemorySwapMax=0`,
  50 signed HTTP/2 peers, 900 seconds per side.
- Runner: `scripts/testbox/run-shared-mesh-resource-e2e.sh`, formal mode,
  baseline locked to `ed109323`, explicit task-owned targets and shared Cargo download cache.
- Source archive SHA-256: `4c6978afcbecc0f97b35ab809847fa3b5cce9d0a6c9caa1a9674bcb944d7c2a7`
- Generated Web archive SHA-256: `2ed6e83db8752bf7808fee0d8da039b18647ddf31b385bbe02b4729871a3444f`
- Baseline archive SHA-256: `36fc1c62ca113d8fefaeae9eee1769174169d0bed524f0599e90e01d68a6899f`
- Baseline/candidate CPU ticks: `236/258`; the integer 5% ceiling was `247`.
- Baseline/candidate total PSS: `31418/30182 KiB`.
- Baseline/candidate anonymous PSS: `16124/18360 KiB`.
- Baseline/candidate file-backed PSS: `17036/16742 KiB`.
- Baseline/candidate stack PSS: `38394/37118 KiB`.
- TLS accepts: `50`; non-H2 requests: `0`; active and peak active connections: `1` per peer.
- Repository summary peak PSS: `27397 KiB`; passed.
- Source journal peak PSS: `27439 KiB`; CPU p95: `0.00%`; extra read bytes: `0`;
  maximum RSS delta: `65536 bytes`; passed.
- Result: `2 passed; 1 failed` in `1861.28s`, runner exit `101`.
- Original manifest and log:
  `/var/folders/nl/qbk0flf9607bv21rd_7d042c0000gn/T/xp-testbox-evidence/`
  `20261002_115625_5607923d807b_f98cc47b.{manifest,log}`.
- Runner cleanup removed its exact run directory and Compose resources; task-owned Cargo
  targets and shared dependency downloads were retained.

The comparable earlier candidate `392f4b2a` passed the same formal resource contract. The delta
introduced a cancellation watch on ordinary response bodies and additional empty overflow-map
checks in the completion worker. The 50-peer fixture returns empty signed bodies, so the dedicated
SSE driver is not exercised. These are measurable optimization candidates, not an established
profile of the failed process. A follow-up repair preserves deadlines, lease cancellation,
completion fairness, and the resource thresholds; only a new full exact-candidate run can
establish acceptance.
