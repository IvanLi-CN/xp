# Classify and roll up every established TCP socket

Connection history used to be limited to business listener ports, so egress sockets were
invisible and control-plane traffic could not be separated from unrelated host processes.
XP now samples every `ESTABLISHED` TCP socket in the node's network namespace once per
minute, classifies it by fixed precedence (business listener, configured control-plane
listener address and port, ingress runtime UID, remaining UID), and no classification branch
drops a row within the inspection bound. `total` is the same-instant sum of the four classes,
and rollups retain per-minute totals so `total` extrema never come from summing per-subject
maxima.

A whitelist would make `total` depend on configuration and would hide the very pressure the
metric exists to explain. The cost is that payload size scales with subject cardinality, not
connection count; this is bounded by binary local encoding, a 16,384-row cap with explicit
`truncated`, and bounded payload sharding. The existing 7-day inbound JSON is imported once as
`inbound` history and kept as a bounded compatibility mirror, so the choice does not discard
already observed data. Daily endpoint-subset totals are approximate by design, while node-level
totals stay exact. Control-plane sockets are classified rather than excluded because excluding
them would silently change `total` when ports move.
