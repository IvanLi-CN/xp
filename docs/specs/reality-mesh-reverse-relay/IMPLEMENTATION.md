# Reality Mesh 反向中继实现状态

## Current Status

- Implementation: core relay path and runtime reconciliation are implemented; deployment/join rollout
  gates remain explicitly closed until their integration evidence exists.
- Lifecycle: active.
- Delivery stop: merge-ready / Step 5C Ready (target; not yet declared).

## Delivered

- 主题分支 `th/reality-mesh-reverse-relay` 从锁定基线创建。
- assignment、wire、生命周期、Xray dynamic API and additive status are implemented. The durable
  state barrier is schema v13, and the coordinator refuses to write the first epoch until every
  voter advertises the assignment capability and at least one candidate reports signed Xray
  readiness plus a managed VLESS endpoint.
- History repository direct sync now tries the assigned Reverse route after Reality/Public direct
  failures and before the existing encrypted dynamic relay.
- Fresh join now returns an additive `reverse_mesh_bootstrap` marker when the assignment capability
  barrier and a managed Rendezvous candidate are available. The leader pre-registers the learner's
  generation/assignment in Raft; `xp join` stores only the public endpoint parameters, epoch and
  generation in the existing mode-0600 `raft_bootstrap_sender` marker. Unsupported or candidate-less
  clusters retain the existing Direct/Public bootstrap path.
- Assignment reconciliation runs a reverse-only signed `health-v2` probe for each assigned target.
  The Rendezvous validates both outer and target ACKs before retaining a bounded health observation;
  local Xray/portal readiness remains the admission gate and a failed probe never disables
  Direct/Public.
- Fresh-join bootstrap links use the domain-separated `ReverseRole::Bootstrap` tag/UUID/origin
  while the durable join operation is active. Both Rendezvous and the learner switch to the
  formal Primary/Standby derivation only after the operation reaches a terminal phase; stale
  bootstrap users/rules then drain for 120 seconds under the same Xray reconciler.
- Mesh status now adds the active Rendezvous role plus the primary and standby members to
  `active_route`. System Status resolves those IDs to current member names and limits every peer
  status cell to at most two single-line summaries. It derives the primary/standby label for each
  direct Rendezvous from current assignments, gives each Reverse target separate `Reverse relay`
  and active Rendezvous/generation lines, counts the local node with all remote members, and
  retains the existing Details entry for the full diagnostic path.
- Fixed Xray spike: `RUN_ID=20260819_102353_be14b3bf_reverse`, Xray `26.3.27`, image digest
  `sha256:592ec4d11f656db95598d01e76dbcc6e002d67360b96a5436500a938230f52c7`. Two Xray
  instances completed dynamic VLESS Reverse registration over both Vision TCP + Reality and
  XHTTP + Reality. The test then proved password SOCKS5, SOCKS-to-Axum H2C prior-knowledge,
  exact-origin routing, unmatched block and rule/outbound removal isolation. The test-only SOCKS
  listener is mapped to a host loopback port because the Rust test runs outside the Xray
  containers; production remains fixed at `127.0.0.1:10086` with no public listener.
- The spike is a transport/protocol gate only. It does not yet prove asymmetric firewall behavior,
  signed end-to-end health, fresh-join bootstrap, deployment restart recovery or the managed-stack
  memory budget; those remain closed integration gates before writing a production epoch.

## Validation

已完成的本地门禁包括 `cargo fmt --all`、`cargo check --all-targets`、`cargo clippy --all-targets -- -D warnings`、
反向 assignment/wire/lifecycle 单测、Web typecheck/lint/unit/build，以及固定 Xray spike。完整
`cargo test`、非对称双节点 Reality transport、fresh join、三种部署升级/回滚、managed-stack
内存 soak、Storybook/E2E、spec drift、独立 review 和 required CI 仍是 Step 5C 的收口条件。
此文件不替代部署真相；部署行为同步到 `docs/ops/**` 与 `AGENTS.md`。
