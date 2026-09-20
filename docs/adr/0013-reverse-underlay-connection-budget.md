# Bound Reverse XHTTP underlay connections

> Status: accepted

US observed one logical HK2 Reverse session with eleven XHTTP TCP underlay connections because
the XP-generated Reverse outbound left XMUX connection reuse at Xray's default unlimited setting.
The system distinguishes a logical `Reverse Link` from its physical `Reverse Underlay Connection`
and from ordinary `User Inbound Session` traffic.

## Decision

Each Reverse Link will use at most two XHTTP underlay TCP connections. Existing connections are
reused and a third socket is not opened. Reverse request admission remains bounded by the existing
per-Rendezvous eight-slot gate; when that gate is full, the request fails fast and uses the existing
Direct/Public fallback rather than queue indefinitely. Assignment changes continue to use the
existing signed Link Lease and 120-second drain; the bound is evaluated after the new generation is
healthy and the retired generation has drained.

The limit is an XP-owned Xray builder default, not an environment variable or Web/API override.
Ordinary user inbound sessions remain a separate metric and are not subject to this Reverse Link
budget.

## Consequences

This caps one control-plane Reverse Link's physical socket fan-out while retaining one spare
connection for weak-network overlap and graceful reuse. Primary and standby Links are bounded
independently. Resource and status evidence must report logical Reverse sessions and physical
underlay connections separately.
