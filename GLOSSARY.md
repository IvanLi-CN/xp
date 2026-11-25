# XP Product Glossary

This glossary defines XP cluster terms and Mihomo subscription vocabulary.

## Language

### Cluster Nodes

**Registered Cluster Node**:
A node recorded in the current XP cluster's node directory.
_Avoid_: Available backend, subscription node

### Control-Plane Mesh

**Direct Mesh**:
A signed control-plane path from one XP node to another over that peer's managed-default
VLESS/Reality endpoint.
_Avoid_: Public Mesh, tunnel Mesh

**Public Fallback**:
The peer's registered public HTTPS `api_base_url` path, used by functional control-plane requests
when Direct Mesh is not selected or unavailable.
_Avoid_: Mesh path, direct peer link

**Direct Connectivity**:
Whether one node can reach another over Direct Mesh, independent of Public Fallback.
_Avoid_: End-to-end reachability, peer availability

**Direct Connectivity Probe**:
A periodic or manual signed health observation that is valid only when it traverses Direct Mesh
and never falls back to Public Fallback. The explicit Direct Mesh enablement preflight is a
separate mechanism; it may use Public Fallback for an owner-approved no-endpoint voter.
_Avoid_: Mesh probe with fallback, public probe

**Public Fallback Probe**:
A separate signed health observation over Public Fallback that measures fallback reachability
without claiming Direct Connectivity.
_Avoid_: Direct probe, Mesh probe

**Active Path**:
The control-plane route XP currently attributes to a peer for functional requests, derived from
functional routing evidence rather than a diagnostic probe.
_Avoid_: Last probe path, public fallback success

**End-to-End Control-Plane Success**:
A functional signed request that reaches its target through any permitted path; it is not evidence
of Direct Connectivity.
_Avoid_: Direct success, Mesh success

### Raft Membership

**DesiredState Node**:
A node recorded in the cluster's authoritative node directory. It describes what the cluster intends
to manage, and it is distinct from whether that node currently holds a Raft identity.
_Avoid_: Cluster member, registered node

**Raft Voter**:
A node that holds a voting identity in the current Raft membership and can therefore participate in
quorum. Every voter maps to exactly one DesiredState Node.
_Avoid_: Node, member

**Orphan Voter**:
A Raft voter with no matching DesiredState Node. It is an incident shape, never steady state.
_Avoid_: Unexpected node

**Absent DesiredState Node**:
A DesiredState Node with no Raft identity. It is valid only as the target of a recorded removal, or
transiently while a single-node quorum recovery has not yet pruned it.
_Avoid_: Deleted node, missing node

**Rejoin**:
Bringing an Absent DesiredState Node back into the Raft membership under its existing identity and
existing endpoints, rather than admitting it as a new node.
_Avoid_: Fresh join, re-register

### Raft Leadership

**Leader Election Handoff**:
An explicit request for one current voter to campaign immediately and be observed as the Raft
leader.
_Avoid_: Target-directed transfer, forced election

### Cluster History

**Repository**:
A node whose Raft membership records it as holding the cluster's long-term history union, together
with the retained records and segments that back that history.
_Avoid_: Database, archive server

**History Record**:
One retained observation of a subject, identified by source, epoch, stream and sequence, and
carrying the observation time range it actually covers.
_Avoid_: Row, sample, event

**Observation Span**:
The time range one History Record covers, from its observation start to its observation end. It is
bounded by the ingestion schedule of its stream and never equal to the query window that returned
it.
_Avoid_: Retention window, sampling period

**Coverage**:
The observed and received time ranges a Repository actually holds. Coverage describes what exists;
it is not a promise about what was measured.

**Gap**:
A time range inside a source stream for which no History Record exists. A Gap is recoverable,
permanent or unclassified, and it is a durable signed fact rather than a transport error.
_Avoid_: Missing data, failure, error

**Completeness** (`complete` / `partial` / `local_only`):
Whether a query result was answered from a completeness-verified Repository, from a Repository with
a relevant permanent Gap, or only from a node's local history window.
_Avoid_: Success, freshness

**Query Window**:
The administrator-chosen time range a history query asks about. It is bounded by the request
contract and is distinct from any Coverage or Gap that the answer reports.
_Avoid_: Range, selection, span

### Subscription Nodes

**Subscription Node**:
A node with an endpoint membership in the current user's subscription.
_Avoid_: Unsubscribed node, invisible node

**Access Point**:
The direct Reality proxy generated for a Subscription Node and exposed as its `*-reality` entry.
_Avoid_: Relay node, landing group

**Landing Group**:
The `🛬 {base}` group for one subscription node base and its direct and chain candidates.
_Avoid_: Access point group, relay group

**Manual Landing Group**:
The `🔒 落地` `url-test` group that health-tests the generated `🛬 {base}` groups.
_Avoid_: Automatic landing pool, access point pool

**Automatic Landing Pool**:
The hidden `🤯 落地` group that health-tests every subscribed `*-reality` Access Point.
_Avoid_: Manual landing group, `🤯 All`

**Empty Fallback**:
The explicit `REJECT` member a provider-derived system group exposes while it has no candidate.
_Avoid_: `COMPATIBLE` placeholder, silent direct

**Escape Direct**:
The trailing `DIRECT` member of `💎 高质量` and `💎 节点选择`, reachable only when no system candidate is available.
_Avoid_: Silent direct, `COMPATIBLE`

**Mihomo Group Interpolation**:
The render-time expansion of reserved entries inside a user's Mihomo proxy group.
It resolves to generated groups and preserves surrounding literal references.
_Avoid_: YAML macro, provider substitution

**Region Placeholder**:
The reserved `🌟 <region>` entry that expands to hidden regional groups such as `🌟 Japan`.
_Avoid_: `🤯 {region}` group, literal wildcard

**Node Placeholder**:
The reserved `🛬 <node>` entry that expands to every available landing group.
_Avoid_: Node selector, relay placeholder

**Relay Group**:
The system `🛣️ {relay-base}` group used by chains; candidates are other nodes' Access Points.
_Avoid_: Landing group, external relay

**Target Node**:
The node a chain proxy ultimately reaches; its own Access Point is excluded from that Relay Group.
_Avoid_: Current node, serving node

### Subscription Import

**Import Source**:
A user-provided subscription URL or copied subscription text that is submitted for node conversion.
_Avoid_: Upstream proxy, source node

**Imported Node**:
A proxy node from an Import Source added to the current user's Mihomo profile.
_Avoid_: Subscription Node, external resource

**Static Node Snapshot**:
Imported Nodes stored in a user's Mihomo profile instead of refreshed from the source.
_Avoid_: Provider, live subscription

**Import Draft**:
A proposed change to a user's Mihomo profile that has not yet been saved.
_Avoid_: Saved profile, provider configuration

**Import Report**:
The outcome of an import attempt: converted nodes plus items that failed or were skipped.
_Avoid_: Import log, provider status

### External Resources

**External Resource**:
A Geo data file or provider document obtained outside XP's generated system provider.
_Avoid_: Subscription node, upstream proxy

**Resource Mirror**:
An XP-served address for an External Resource that lets Mihomo obtain the resource through XP.
_Avoid_: Generic reverse proxy, cached copy

**Mirror Mode**:
The explicit delivery mode that replaces eligible External Resource URLs with Resource Mirror URLs.
_Avoid_: Proxy mode, relay mode

**Original Resource URL**:
The source address for an External Resource before Mirror Mode is applied.
_Avoid_: Mirror URL, source node

**GeoX Resource**:
A Mihomo Geo data resource named in `geox-url`, such as `geoip`, `geosite`, `mmdb`, or `asn`.
_Avoid_: Geo proxy, Geo node

**Resource Directory**:
The set of External Resources currently available through XP's Resource Mirror addresses.
_Avoid_: Resource cache, URL registry

**Canonical Mihomo Subscription**:
The user-facing Mihomo configuration that references XP's system provider.
_Avoid_: System provider payload, raw subscription

**System Provider Payload**:
XP's generated proxy document loaded by the Canonical Mihomo Subscription.
_Avoid_: Canonical subscription, external resource

**Mirror Origin**:
The public HTTP origin used in a subscription response to construct Resource Mirror addresses.
_Avoid_: Upstream origin, cluster primary

**Mirrorable Resource**:
An External Resource eligible for access through XP's Resource Mirror.
_Avoid_: Any URL, proxied resource

## DDNS

**DDNS Family Synchronization**:
Successful detection of a public address and synchronization of its DNS record.
Both occur in the current reconciliation round.
_Avoid_: Client reachability, historical address

**DDNS Health**:
The status of DDNS reconciliation.
It is up when a family completes DDNS Family Synchronization in the current round.
_Avoid_: Universal client availability

## Web Demo

**Demo Runtime**:
An explicit Web build/runtime that uses the formal XP page tree with an in-memory mock backend and never falls back to a live backend.
_Avoid_: Demo mode query switch, fake page tree

**Demo Scene**:
A deterministic dataset and fault profile selected by the Demo URL and rebuilt in memory.
_Avoid_: Demo fixture page, saved demo workspace

**Demo Persona**:
A simulated authentication state used to exercise the formal permission surface, such as administrator or unauthenticated.
_Avoid_: Demo role, demo account

**Demo Inspector**:
The local-only control surface for Demo scene, persona, network, data, actions and share state.
_Avoid_: Product settings, query-string toolbar

**Demo Share State**:
The allowlisted URL parameters that reproduce a Demo scene, persona, fault profile and product page without carrying secrets or mutation payloads.
_Avoid_: Serialized demo state, shareable credentials

## Web Admin

**Primary Backend**:
The single same-cluster HTTPS node origin that receives the browser's API and
status-stream requests for the current Web/PWA session.
_Avoid_: Service node, current node, automatic failover node

**Backend Candidate**:
A same-cluster HTTPS origin already known to the browser from the current page,
an authenticated node profile, or an approved static-console runtime policy and
eligible for manual selection.
_Avoid_: Arbitrary backend, relay node

**Pre-auth Backend Selection**:
A temporary Primary Backend choice made on the Login page before admin-token
verification; it becomes durable only after authentication succeeds.
_Avoid_: Login server switch, automatic failover

## Native Data Plane

**xp-proxy**:
XP 的独立 Rust 代理数据面程序；在对比测试中以正式程序名区分于 Xray。
_Avoid_: 将 native、native64 当作正式程序名称

**Mainline Performance Baseline**:
与候选程序同场测量的主干 xp-proxy 对照程序；其版本在一次测试期间保持固定。
_Avoid_: 历史性能数值、最近发布版本

**Performance Gate Waiver**:
人工允许某个 PR 不受性能测试结论阻止合并的决定；不改变测试执行、真实结果或结果有效性。
_Avoid_: 跳过性能测试、性能通过

**Invalidated Performance Result**:
被人明确作废、保留原始记录但不再参与有效结果汇总或复用的一次性能测试结果。
_Avoid_: 测试失败、版本禁测、门禁豁免

**Performance Sample**:
一次性能验收中独立完成的冷/热成对测量；每个 sample 使用自己的缓存命名空间，因此
不与其他 sample 或上一次 attempt 共享缓存。
_Avoid_: 重试、平均值

**Required Gate**:
合并前必须成功的检查集合；只有 Required Gate 受墙钟预算约束，验收测量与发布流程不受。
_Avoid_: 全部 CI、建议检查

**Resource Evidence**:
在 128 MiB/no-swap cgroup 内针对某个精确 commit 取得的内存与吞吐证明，以及绑定该证明
与其工作负载日志的摘要。
_Avoid_: 性能测试、历史基线报告

**Evidence Commit**:
仅父提交为目标 commit、仅含证据文件、并由仓库 owner 签名的一份独立提交。
_Avoid_: 证据附件、release tag

**Measurement Cache**:
按运行与 attempt 命名、仅服务于单次验收测量、事后必须回收的 Actions 缓存条目。
_Avoid_: 共享构建缓存、长期缓存

**Control Plane**:
The XP authority that stores and coordinates cluster, endpoint, user and membership state.
It produces the complete desired snapshot for a node's data plane.
_Avoid_: The process that accepts proxy traffic

**Data Plane**:
The runtime that accepts external proxy sessions, authenticates them, forwards traffic and
reports runtime state. In this topic the native data plane is `xp-proxy`; Xray remains a separate
data-plane implementation.
_Avoid_: Control plane, endpoint metadata

**Proxy Snapshot**:
A complete, versioned description of the listener, user, protocol, fallback and runtime settings
that XP authoritatively publishes to the local data plane.
_Avoid_: Partial config patch, local proxy cache

**Ingress Owner**:
The explicit data-plane choice for an endpoint: `xray` or `native`. It is independent from the
transport choice such as `xhttp`, and exactly one owner may hold an endpoint port at a time.
_Avoid_: Installed binary, active process

**Generation**:
The durable XP ordering value attached to a Proxy Snapshot. It lets XP and `xp-proxy` distinguish
the current snapshot, a stale snapshot and an explicitly authorized rollback.
_Avoid_: Process start time, request id

**Prepared Generation**:
A candidate snapshot that passed validation but does not yet own a public listener or accept new
external sessions.
_Avoid_: Ready ingress, active generation

**Active Generation**:
The snapshot whose listeners currently own their endpoint ports and are authorized to accept new
sessions.
_Avoid_: Prepared generation, installed release

**Drain**:
The bounded lifecycle period during which a data plane stops accepting new sessions while existing
sessions finish or are closed at the confirmed deadline.
_Avoid_: Immediate stop, automatic failback

**Usable Ingress Point**:
An endpoint with explicit native owner, verified node capability, current active generation, all
listeners bound, self-test passed, and proof that no other runtime owns the same port.
_Avoid_: Binary present, service process alive

**Compatibility Baseline**:
The exact Xray/Mihomo versions and fixtures against which native protocol and deployment behavior
are accepted. Current native design evidence is bound to Xray `26.3.27` and Mihomo `v1.19.29`.
_Avoid_: Any recent client, version range without fixtures

## Node Connections

**Connection Count**:
The number of TCP sockets in one Connection Class at one sampling instant.
_Avoid_: Active connection, session count

**Connection Total**:
The same-instant sum of all Connection Classes for one node.
_Avoid_: Aggregate connection, session total

**Connection Class**:
One mutually exclusive attribution category of a sampled connection: inbound, control plane,
runtime outbound, or other outbound.
_Avoid_: Connection type, traffic direction

**Inbound Connection**:
An `ESTABLISHED` TCP socket whose local port is a business endpoint listen port.
_Avoid_: Accepted session, online connection

**Outbound Connection**:
An `ESTABLISHED` TCP socket without a business listener, attributed by its Connection Class.
_Avoid_: Egress session, user connection

**Control-plane Connection**:
A connection whose matching side equals a configured XP/Xray control listener address and port.
_Avoid_: API call, internal session

**Connection Rollup**:
The min/avg/max/sample-count aggregate of instantaneous connection samples inside a time bucket.
_Avoid_: Average session, connection total
