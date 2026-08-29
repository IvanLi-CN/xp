# Managed Host Operations Context

This glossary defines the language for XP-managed systemd and OpenRC hosts. It
describes ownership and runtime meaning, not firewall syntax or deployment steps.

## Ingress Protection

**Ingress Guard**:
A root-installed kernel ruleset that constrains new public connections delivered
to Xray without giving the `xp` service host-administration privileges.
_Avoid_: guard daemon, XP firewall process

**Protected Xray Socket**:
Every non-loopback TCP listening socket owned by the Xray service cgroup.
Loopback-only control-plane sockets are not Protected Xray Sockets. The term
does not distinguish XP-managed endpoint ports from other public Xray
listeners.
_Avoid_: configured VLESS port, static port list

**Guard Refresh Hook**:
A root-owned, short-lived service-start action which runs after the Xray
service cgroup exists and before Xray executes. It atomically binds the
Ingress Guard to that current cgroup, verifies the owned ruleset, and exits.
It accepts no `xp`-controlled firewall input and is not a resident process.
_Avoid_: guard daemon, privilege handoff to XP

**Enforced Guard**:
An enabled Ingress Guard whose Refresh Hook must establish a Verified Guard
Ruleset before Xray may start. A refresh failure prevents that Xray start; it
never silently falls back to an unprotected public listener.
_Avoid_: unprotected fallback, best-effort enforcement

**Guard Activation**:
A root-only `xp-ops` operation which validates host support before it writes
assets or changes the owned ruleset, then restarts Xray once into an Enforced
Guard. Existing host-managed nodes use this immediate path; observation is an
explicit diagnostic mode rather than a required rollout phase.
_Avoid_: implicit XP action, partial firewall install

**Admission Drop**:
A silent discard of an over-limit initial TCP SYN before the packet reaches a
Protected Xray Socket. It neither creates an Xray connection nor emits a TCP
reset.
_Avoid_: post-handshake disconnect, reset-on-limit

**Small VPS Guard Profile**:
The immediate-enforcement profile for a constrained node: all Protected Xray
Sockets share eight new TCP SYNs per second with a burst of twenty; each IPv4
or IPv6 source has three per second with a burst of eight. Existing connections
are outside this admission budget.
_Avoid_: connection cap, XHTTP stream limit

**Verified Guard Ruleset**:
The last Ingress Guard ruleset that the kernel accepted and whose owned objects
can be read back as the expected configuration.
_Avoid_: desired rules, cached firewall config
