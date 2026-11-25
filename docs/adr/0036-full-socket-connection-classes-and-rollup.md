# Classify and roll up every established TCP socket

Connection history used to be limited to business listener ports, so egress sockets were
invisible and control-plane traffic could not be separated from unrelated host processes.
XP now samples every `ESTABLISHED` TCP socket in the node's network namespace once per
minute, classifies it by fixed precedence (business listener, control-plane port, ingress
runtime UID, remaining UID), and never drops a row. `total` is the same-instant sum of the
four classes, and rollups retain per-minute totals so `total` extrema never come from summing
per-subject maxima.

A whitelist would make `total` depend on configuration and would hide the very pressure the
metric exists to explain. The cost is that payload size scales with subject cardinality, not
connection count; this is bounded by binary local encoding, a 16,384-row cap with explicit
`truncated`, and bounded payload sharding. Daily endpoint-subset totals are approximate by
design, while node-level totals stay exact. Control-plane sockets are classified rather than
excluded because excluding them would silently change `total` when ports move.
