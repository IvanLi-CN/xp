# Host-Managed Fresh-Join Evidence

- Candidate commit: `1e54d319f92d0d6f3db90d27fa0be52cb04f5d67`
- Testbox run: `20260930_231616_1796485043_1e54d319`
- Runner: `XP_TEST_IMAGE=xp-xray-base-cc55ad0d scripts/testbox/run-host-managed-fresh-join-e2e.sh`
- Result: passed.
- Receipt: `host-managed-fresh-join-codex_xp__4e2c0b5b_host_join_20260930_231616_1796485043_`
  `1e54d31.txt`
- Observed `deploy=official-xp-ops` with `leader`, `systemd`, and `openrc`; both joined nodes
  reported `follower`.
- The run verified durable metadata, `/etc/xp/xp.env`, the persisted admin-token hash, active
  systemd/OpenRC services, service restart identity preservation, OpenRC XP `SIGKILL` respawn,
  and the leader node list containing all three node names.
- The testbox had been restarted and no default candidate image was available, so the runner used
  an agent-created temporary Xray-only base image. The image and its workspace were removed after
  the run; no production node or persistent project data was touched.
- This is supplementary A4 evidence for host-managed membership and service recovery. The runner
  does not expose committed index, `last_applied`, election-timeout counters, or controlled
  Mesh-gate/body contention; those claims remain covered by deterministic local regressions and
  are not claimed as measured by this run.
