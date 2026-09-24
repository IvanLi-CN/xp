# Spec Catalog

`docs/specs/` holds the durable product and operations contracts for XP topics.
Each topic uses a stable lowercase-kebab-case directory and contains:

- `SPEC.md`: background, requirements, contracts, acceptance criteria, risks, and references.
- `IMPLEMENTATION.md`: delivery coverage, rollout state, and remaining gaps.
- `HISTORY.md`: topic-local lifecycle, replacement, and compatibility history.

## Lifecycle

- `active`: the topic remains a current contract.
- `superseded`: a successor owns future changes; the retained topic remains historical context.
- `archived`: a completed or retired topic with no active delivery scope.

## Topics

| Topic                 | Lifecycle  | Implementation | Spec       | Successor  | Notes |
| --------------------- | ---------- | -------------- | ---------- | ---------- | ----- |
| Access model          | active     | not started    | [Spec][1]  | -          |       |
| Admin delete          | active     | complete       | [Spec][2]  | -          |       |
| CI YAML fix           | archived   | complete       | [Spec][3]  | -          |       |
| Cloudflare Tunnel     | active     | complete       | [Spec][4]  | -          |       |
| History repositories  | active     | in progress    | [Spec][5]  | -          |       |
| Node onboarding       | active     | complete       | [Spec][6]  | -          |       |
| IP geo                | active     | complete       | [Spec][7]  | -          |       |
| Shared node list      | active     | complete       | [Spec][8]  | -          |       |
| Docker                | active     | complete       | [Spec][9]  | -          |       |
| Mihomo SMux           | active     | complete       | [Spec][10] | -          |       |
| Probe participants    | active     | complete       | [Spec][11] | -          |       |
| Probe self-test       | active     | complete       | [Spec][12] | -          |       |
| VLESS XHTTP           | active     | complete       | [Spec][13] | -          |       |
| Style gates           | active     | in progress    | [Spec][14] | -          |       |
| Fixture policy        | active     | complete       | [Spec][15] | -          |       |
| Managed memory        | active     | not started    | [Spec][16] | -          |       |
| Private CIDRs         | active     | complete       | [Spec][17] | -          |       |
| Mihomo provider       | active     | complete       | [Spec][18] | -          |       |
| Mihomo redact         | active     | complete       | [Spec][19] | -          |       |
| Mihomo template       | active     | complete       | [Spec][20] | -          |       |
| Node history          | active     | complete       | [Spec][21] | -          |       |
| Node list             | active     | complete       | [Spec][22] | -          |       |
| Node panel            | superseded | complete       | [Spec][23] | [Spec][22] |       |
| Runtime fan-out       | active     | complete       | [Spec][24] | -          |       |
| Service observability | active     | complete       | [Spec][25] | -          |       |
| TCP connections       | active     | complete       | [Spec][26] | -          |       |
| Inbound IP usage      | active     | complete       | [Spec][27] | -          |       |
| Traffic analytics     | active     | complete       | [Spec][28] | -          |       |
| OpenRC recovery       | active     | complete       | [Spec][29] | -          |       |
| Prerelease E2E        | active     | unverified     | [Spec][30] | -          |       |
| Quota ratios          | active     | complete       | [Spec][31] | -          |       |
| Raft membership       | active     | complete       | [Spec][32] | -          |       |
| Single-node recovery  | active     | complete       | [Spec][33] | -          |       |
| Reality fallback      | active     | in progress    | [Spec][34] | -          |       |
| Mesh reverse relay    | active     | in progress    | [Spec][35] | -          |       |
| Release alerts        | active     | complete       | [Spec][36] | -          |       |
| Release intent        | active     | in progress    | [Spec][37] | -          |       |
| Remove Grant groups   | active     | complete       | [Spec][38] | -          |       |
| Remove Grants         | active     | complete       | [Spec][39] | -          |       |
| Resource monitoring   | active     | not started    | [Spec][40] | -          |       |
| Scroll containers     | active     | complete       | [Spec][41] | -          |       |
| Service log rotation  | active     | not started    | [Spec][42] | -          |       |
| shadcn/ui migration   | active     | complete       | [Spec][43] | -          |       |
| Service monitoring    | active     | complete       | [Spec][44] | -          |       |
| Web local upgrade     | active     | complete       | [Spec][45] | -          |       |
| Web primary backend   | active     | complete       | [Spec][46] | -          |       |
| Web PWA               | active     | complete       | [Spec][47] | -          |       |
| Web resilience        | active     | in progress    | [Spec][48] | -          |       |
| Web tools             | active     | complete       | [Spec][49] | -          |       |
| xp-ops admin token    | active     | complete       | [Spec][50] | -          |       |
| Control-plane relay   | superseded | retired        | [Spec][51] | [Spec][34] |       |
| Xray ingress guard    | active     | complete       | [Spec][52] | -          |       |
| Xray stale TCP        | active     | complete       | [Spec][53] | -          |       |

[1]: access-model-hardcut-v9/SPEC.md
[2]: admin-delete-node/SPEC.md
[3]: ci-workflow-yaml-parse-fix/SPEC.md
[4]: cloudflare-tunnel-remote-access/SPEC.md
[5]: cluster-history-repositories/SPEC.md
[6]: cluster-node-onboarding/SPEC.md
[7]: country-is-ip-geo/SPEC.md
[8]: dashboard-nodes-shared-list/SPEC.md
[9]: docker-single-image-cluster-node-deploy/SPEC.md
[10]: endpoint-mihomo-smux/SPEC.md
[11]: endpoint-probe-online-participants/SPEC.md
[12]: endpoint-probe-skip-self-test/SPEC.md
[13]: endpoint-vless-xhttp-reuse/SPEC.md
[14]: engineering-style-gates/SPEC.md
[15]: fixture-policy-gate/SPEC.md
[16]: managed-stack-64m-memory/SPEC.md
[17]: mihomo-private-resource-cidrs/SPEC.md
[18]: mihomo-provider-dual-track/SPEC.md
[19]: mihomo-redact-cli/SPEC.md
[20]: mihomo-subscription-template/SPEC.md
[21]: node-history-fallback/SPEC.md
[22]: node-list-cross-origin-actions/SPEC.md
[23]: node-panel-link-entry/SPEC.md
[24]: node-runtime-fanout-timeout-tolerance/SPEC.md
[25]: node-service-observability/SPEC.md
[26]: node-tcp-connection-count/SPEC.md
[27]: node-user-inbound-ip-usage/SPEC.md
[28]: node-user-traffic-analytics/SPEC.md
[29]: openrc-low-memory-runtime-recovery/SPEC.md
[30]: prerelease-e2e-verification/SPEC.md
[31]: quota-policy-node-ratio-editor/SPEC.md
[32]: raft-membership-voter-invariant/SPEC.md
[33]: raft-single-node-recovery/SPEC.md
[34]: reality-fallback-control-plane-mesh/SPEC.md
[35]: reality-mesh-reverse-relay/SPEC.md
[36]: release-failure-telegram-alerts/SPEC.md
[37]: release-intent-governance/SPEC.md
[38]: remove-grant-groups-hard-cut/SPEC.md
[39]: remove-grants-hard-cut/SPEC.md
[40]: resource-monitoring/SPEC.md
[41]: scroll-containers/SPEC.md
[42]: service-log-rotation/SPEC.md
[43]: shadcn-ui-full-migration/SPEC.md
[44]: uptime-monitoring/SPEC.md
[45]: web-local-upgrade/SPEC.md
[46]: web-primary-backend-failover/SPEC.md
[47]: web-pwa-offline-console/SPEC.md
[48]: web-runtime-resilience/SPEC.md
[49]: web-tools-mihomo-redact/SPEC.md
[50]: xp-ops-admin-token-ops/SPEC.md
[51]: xray-control-plane-relay/SPEC.md
[52]: xray-ingress-guard/SPEC.md
[53]: xray-stale-tcp-reclaim/SPEC.md
