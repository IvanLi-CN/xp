# Resource Monitoring

This glossary defines bounded resource observations for XP-managed node
runtimes. It distinguishes runtime resource evidence from remote Service
Monitoring and from an unrestricted metrics platform.

## Scope

**Resource Monitoring**:
The XP context for observing resource use by a node's Managed Runtime Stack and
its Resource Domain over time.
_Avoid_: generic metrics platform, service monitoring, system status

**Managed Runtime Stack**:
The XP, Xray, cloudflared, and XP-owned canary runtimes that XP manages for one
node.
_Avoid_: arbitrary host process, application fleet

**Resource Domain**:
The execution boundary whose aggregate resources are observed: the host for a
host-managed node, or the workload cgroup for an official single-image
container node.
_Avoid_: host metrics, container host

## Evidence

**Resource Sample**:
A bounded point-in-time reading of resource measurements for one Resource
Domain and its Managed Runtime Stack.
_Avoid_: probe, event, process inventory

**Resource Rollup**:
A time-bucketed summary of Resource Samples that retains measurement meaning
and capture completeness.
_Avoid_: raw sample, status history

**Resource History Stream**:
The ordered history authored by a node for durable Resource Rollup storage.
_Avoid_: Raft state, log export

**Resource Store**:
The node-local durable record of Resource Rollups, capture gaps, and Resource
Alert state before history delivery.
_Avoid_: History Repository, raw sample buffer

**Resource Completeness**:
The explicit indication of whether every expected Resource Sample contributed
to a Resource Rollup.
_Avoid_: zero utilization, healthy node

**Measurement Capability**:
The supported, partial, or unsupported ability of one Resource Domain to
provide a named resource measurement without elevated XP privileges.
_Avoid_: best-effort zero, permission error as utilization

**Resource Capture Suspension**:
The explicit state in which Resource Rollups cannot be durably accepted for
history delivery while current in-memory readings remain available.
_Avoid_: silent retention reduction, historical zero

**Resource Alert**:
A locally derived administrative signal that a resource condition has remained
outside its configured operating boundary or has recovered.
_Avoid_: external notification, incident

**Resource Alert Policy**:
The cluster default resource thresholds together with an explicit node or
Managed Runtime Stack override.
_Avoid_: automatic tuning, host-specific hidden default
