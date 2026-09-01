# Service Monitoring

This glossary defines administrator-managed remote availability checks observed
from XP cluster nodes. It separates configured monitoring intent from each
node-local execution.

## Language

**Service Monitoring**:
The XP context for defining remote availability checks and interpreting their
observations over time.
_Avoid_: system status, endpoint probe, node observability

**Service Monitor**:
The durable cluster definition of one remote target, check kind, schedule, and
observer policy. It is monitoring intent, not a check result.
_Avoid_: service, endpoint, probe

**Observer Node**:
A cluster node selected to execute a Service Monitor from its own network
position.
_Avoid_: agent, runner, probe node

**Observer Set**:
The capable Observer Nodes from which a Service Monitor is expected to run.
The default set contains every capable cluster node and may be narrowed by the
administrator.
_Avoid_: replica set, worker pool

**Schedule Slot**:
One expected execution of a Service Monitor by one Observer Node at a fixed UTC
time. A Schedule Slot exists even when no check result is captured.
_Avoid_: cron run, sample

**Observation**:
The captured result of one Schedule Slot, including its outcome and bounded
measurements. It is historical evidence, not the current Monitor status.
_Avoid_: probe, health, event

**Public Target**:
A Service Monitor target whose complete resolution and redirect path contains
only globally routable addresses.
_Avoid_: unrestricted target, private target

**Observation Capture Suspension**:
The explicit state in which due checks are not started because their Source
cannot durably accept new Observations for History Repository delivery.
_Avoid_: target down, monitor disabled, silent data loss

**Observation Budget**:
The cluster-wide admission limit on expected Schedule Slots per minute. It is
computed from active Service Monitors and their Observer Sets.
_Avoid_: request rate, History Repository quota

**Monitor Revision**:
An immutable version of a Service Monitor configuration. An edit creates a new
revision that takes effect at the next Schedule Slot; captured Observations keep
the revision that produced them.
_Avoid_: mutable monitor state, migration version

**Ad Hoc Observation**:
A manually requested check result. It is retained and displayed with history,
but is excluded from scheduled availability and coverage calculations.
_Avoid_: retry, scheduled sample

**Availability**:
The proportion of successful executable Observations for a selected period.
Unsupported, suspended, and otherwise uncaptured Schedule Slots do not enter
the denominator.
_Avoid_: uptime percentage when coverage is unknown

**Coverage**:
The proportion of expected scheduled Slots that produced an executable
Observation. It exposes monitoring blind spots independently of Availability.
_Avoid_: data completeness without a denominator

**Current Status**:
The aggregate of the latest complete scheduled slot for each selected Observer
Node: `up` when all valid results pass, `down` when all valid results fail,
`degraded` when results are mixed, and `unknown` when no valid result exists.
`capture_suspended` is a separate capture state, not a target outcome.
_Avoid_: incident, alert state

**Monitor Lifecycle**:
The administrator-controlled state of a Service Monitor: `active` schedules
checks, `paused` intentionally schedules none while retaining history, and
`deleted` schedules no future checks while retained history ages normally.
_Avoid_: maintenance window, incident state

**Observation Quality**:
The repository-backed description of historical completeness, including
coverage, watermark, gaps, and whether data is `complete`, `partial`, or
`local_only`.
_Avoid_: confidence score, health status

**Capture Budget**:
The admission guard that bounds scheduled executions and separate ad-hoc
executions. A budget rejection is a configuration error, not a reduced
Observer Set.
_Avoid_: best-effort throttling, silent sampling reduction
