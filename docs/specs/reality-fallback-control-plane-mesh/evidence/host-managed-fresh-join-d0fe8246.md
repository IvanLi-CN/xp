# Host-Managed Fresh-Join Evidence

- Current implementation candidate: `d0fe824625d5e58fca8455b26d5d3032bf9a45fb`
- Same behavior baseline run: `c4e899f702c464b2f689e9ac9cdf68c1fac5211c`
- Testbox run: `20261001_010148_3176706633_c4e899f`
- Runner result: passed with `deploy=official-xp-ops`, one leader, systemd follower, and OpenRC
  follower; restart identity was preserved and OpenRC XP `SIGKILL` respawn was verified.
- The current candidate's only changes after that run are the success-path telemetry timer
  optimization in `src/control_plane_mesh/gate.rs` and `src/control_plane_mesh/telemetry.rs`;
  `git diff --name-only c4e899f7 d0fe8246` is limited to those two files. No host-managed runner,
  membership, persistence, or deployment code changed.
- This card is supplementary host-managed evidence, not a same-SHA receipt. The current SHA's
  same-SHA empirical gate is recorded in `mesh-resource-d0fe8246.md`; no production node or
  persistent testbox project data was touched.
