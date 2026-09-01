# Persist observations through the source journal to History Repository

Observer Nodes are the execution sources for Service Monitoring. They create
structured Observations locally, enqueue them in the existing Source Delivery
Journal, and deliver signed cursor-based segments to History Repository nodes.
Monitor definitions and scheduler metadata remain Raft-backed, while complete
observations and semantic rollups live in the repository's history store.

The Source Delivery Journal has a bounded disk budget. When it cannot accept a
new Observation, the scheduler enters `capture_suspended` and does not start
new due checks until delivery capacity returns. It never continues probing and
discards results, and it never creates an ordinary-node long-term history
fallback. Repository query responses must preserve the existing quality metadata
(`complete`, `partial`, or `local_only`) and coverage/gap watermarks.
