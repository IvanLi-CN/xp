# xp-proxy perf gate: A/A noise and rotated-round decisions

## Symptoms

2026-11-24 push run `45399956338` failed the light `xp-proxy-perf` measurement while the candidate
and the baseline were the **same commit** (`d7871a83`). The summary header said
`Candidate: d7871a83…; main: d7871a83…`, yet `inbound/c8/upload` was reported as
`2738.96 Mbps` against `3118.19 Mbps` (-12.2%). The opposite-order sweep of the same run measured
`3025.18` against `3164.75` (-4.4%), so the failure came from one noisy sweep.

The same harness had already failed the same scenario on pull-request run `45394279560`
(`418e45f5` against main `addcb231`) with ratios `0.866` and `0.930`. That difference disappeared
after the single-resolved-address direct-connect change (`45397132972`, ratios `0.981` and `0.996`).

## What the A/A data shows

Per-round candidate/main ratios from run `45399956338` (identical binaries):

| Scenario            | Round 0 | Round 1 | Median |
| ------------------- | ------: | ------: | -----: |
| inbound/c1/latency  |   0.988 |   0.977 |  0.983 |
| inbound/c8/latency  |   1.014 |   1.017 |  1.016 |
| inbound/c1/upload   |   1.011 |   1.010 |  1.011 |
| inbound/c8/upload   |   0.878 |   0.956 |  0.917 |
| inbound/c1/download |   1.003 |   0.999 |  1.001 |
| inbound/c8/download |   1.015 |   0.993 |  1.004 |

Only `c8/upload` moves: 7.8 percentage points between two sweeps of one binary. The cause is
visible in the resource samples. At `c8` the proxied subject saturates roughly one core
(`main` 89.2% and 90.3%, `candidate` 81.4% and 93.5%), so throughput follows CPU time almost
linearly and any cold-start or scheduling dip maps directly into the result. In the failing round
the candidate used 81.4% of a core instead of ~93%. The direct origin control measured
`6125.5 Mbps` upload, so the origin and generator had headroom; the `direct capacity` check did not
fire.

The order was also fixed: round 0 ran `candidate, main, Xray`, round 1 ran `Xray, main, candidate`.
The candidate therefore always paid the first-measured, cold-run position, and main always ran
second. On run `45394279560` the first-measured candidate again showed the low CPU utilization
(85.9% against 100.6% for main); on run `45397132972` it did not, and both rounds passed.

## Why the old rule misreported this

- Both sweeps had to pass against both baselines: four independent single-slot chances to trip per
  scenario. A 7.8% A/A swing between rounds is larger than the 10% margin the check assumes.
- Fixed order gave the candidate a systematic cold-start disadvantage in one of the two rounds.
- The report kept per-sweep numbers but no dispersion, so an identical-binary run could not be
  recognized as noise.
- `c8/upload` is the only cell close to CPU saturation, so it carries most of the measurement noise
  and needed more samples than the other cells.

## Decision rule now used

- Light profile measures **three rotated rounds**: `candidate,main,Xray`, then
  `main,Xray,candidate`, then `Xray,candidate,main`. Every subject occupies every start position
  once. Full keeps the two opposite-order rounds because its 360s design budget cannot absorb a
  third full matrix; it stays advisory, never the merge gate.
- Per scenario, the decision uses the **median of the paired per-round values**: throughput ratio
  `candidate/baseline`, latency delta `candidate - baseline` for p50 and p95. The fixed contract
  thresholds are unchanged at 90% and +5 ms.
- With three rounds, "median below 90%" means "at least two of three rounds below 90%". One noisy
  round can no longer fail a scenario, while a sustained regression that shows in a majority of
  rounds still fails.
- `upload@8` runs three 2s repetitions inside each round and keeps their median. Repeat validity
  is still the worst repeat: any error, empty sample set, or missing 1 MiB of confirmed payload
  fails the cell.
- The direct origin control must have valid samples and stay 20% above the fastest proxied result
  for upload and download.
- `measurement.json` and `summary.md` now carry a noise table: per-round ratios/deltas, their
  median, and their spread. A same-SHA run is labelled as an A/A noise sample instead of an
  independent regression comparison.
- No retries were added, the thresholds were not relaxed, `measurement: failure` and the human
  `perf:waive` path are untouched, and a missing round is a failure.

## Evidence

Replaying the previously failing artifact through the new decision shows the intended split:

- run `45399956338` (A/A, ratios `0.878`, `0.956`): median `0.917`, passes.
- run `45394279560` (sustained dip, ratios `0.866`, `0.930`): median `0.898`, still fails as
  `inbound/c8/upload: throughput below 90% of main (median round ratio 0.898)`.

Three A/A dispatches of the light profile on this branch measured `candidate_sha = main`
(`d7871a83`) with the new sampling. Each run made 9 paired rounds per scenario:

| Run         | c8/upload rounds    | Median | vs Xray | Result  |
| ----------- | ------------------- | -----: | ------: | ------- |
| 45425807066 | 0.996, 1.063, 0.958 |  0.996 |   1.023 | success |
| 45425882080 | 0.990, 0.990, 1.024 |  0.990 |   1.124 | success |
| 45426029831 | 0.900, 1.012, 1.019 |  1.012 |   1.132 | success |

Across the three runs, every throughput median stayed within 0.977..1.008 of main for the
`c1/upload`, `c1/download`, `c8/download` cells, and within 0.990..1.012 for `c8/upload`. The
largest single-round deviation was `0.9002` (-10.0%) in run `45426029831`, and that run's median
still passed at `1.012`; the worst round in the other runs was 0.958. This is the noise the old
every-round rule turned into merge failures. The largest median latency
delta was -0.049ms p95 against Xray, far inside the +5ms bound. All 16 decisions (four
throughput scenarios and p50/p95 for two latency workloads, each against main and Xray)
passed in all three runs, and no run reported a budget warning at 240s.
Consumer wall clock was 146.5s to 152.2s against the 240s design budget. Another run on the
final head is produced by the pull request itself and exercises the same decision path with
different build identities.

Deterministic coverage in `scripts/perf/xp-proxy/test_policy.py`:

- rotation gives every subject every start position;
- a single depressed round passes and a majority of depressed rounds fails;
- the recorded A/A numbers pass while the same shape at -15% fails;
- latency uses the median p50/p95 delta and reports a missing sample;
- direct headroom rejects an invalid control and a control below 120%;
- repeated-cell aggregation keeps the worst repeat's samples, confirmed bytes, and errors.

## Limits

The runner is a shared 4-core GitHub-hosted VM with no exclusive cores. Residual `c8/upload` noise
remains, so a true regression of almost exactly 10% sits on the decision boundary; the gate is
designed to reject single-round noise and to catch sustained degradation, not to resolve an
idealised 10.0% change. Wall clock grows: light sampling is 144s instead of 72s and the light
design budget is 240s instead of 180s. No jobs, artifacts, or runs were added; the extra time is
the cost of a stable merge gate. Full stays within 360s.

## Where the change lives

- `scripts/perf/xp-proxy/benchmark.py`: `orders`, `combine`, `compare`, `headroom`, `noise`, and the
  round/repeat loop in `main`.
- `scripts/perf/xp-proxy/config.json`: `rounds` per profile and `upload@8` repeats.
- `scripts/perf/xp-proxy/test_policy.py`: deterministic A/A, regression, latency, headroom, and
  repeat coverage.
- `scripts/perf/xp-proxy/timing.py`: light design budget 240s.
- `scripts/perf/xp-proxy/README.md`,
  `docs/specs/native-vless-xhttp-proxy/contracts/performance.md`, and
  `docs/specs/native-vless-xhttp-proxy/IMPLEMENTATION.md`: the decision rule, sampling matrix, and
  budget as current project truth.
