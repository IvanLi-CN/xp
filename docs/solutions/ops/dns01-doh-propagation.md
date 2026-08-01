---
title: DNS-01 DoH propagation checks
module: vless HTTPS canary
problem_type: certificate issuance and renewal
component: Cloudflare DNS-01 solver
tags: [acme, dns-01, doh, cloudflare, vless-canary]
status: active
related_specs: [3e4q4-mihomo-provider-dual-track]
---

# DNS-01 DoH Propagation Checks

## Context

The managed VLESS HTTPS canary obtains certificates with Cloudflare DNS-01.
Some host-managed nodes can make outbound HTTPS requests but cannot directly query authoritative
nameservers on UDP or TCP port 53.

## Symptoms

Certificate issuance or renewal fails after Cloudflare accepts the TXT record, while direct
authoritative DNS queries time out from the node.

## Root Cause

Using direct authoritative DNS queries as the local propagation gate makes certificate renewal
depend on a network path that ACME DNS-01 does not require.

## Resolution

Use both Cloudflare and Google DNS-over-HTTPS resolvers for TXT propagation checks. Keep the
Cloudflare API as the only DNS-01 record writer and retain the configured propagation timeout.
Treat negative answers from an individual resolver upstream as retryable so cached NXDOMAIN or
NODATA responses do not end the propagation check early.

## Guardrails

- Do not replace DNS-01 with HTTP-01 or TLS-ALPN-01 for this canary.
- Do not require node-to-authority UDP/TCP 53 reachability for the propagation gate.
- Do not trust a single DoH resolver upstream's negative response as the final visibility result.
- Treat either DoH resolver failing to show the expected TXT before timeout as not propagated.

## References

- `src/vless_https_canary.rs`
- `docs/specs/3e4q4-mihomo-provider-dual-track/SPEC.md`
- `docs/ops/README.md`
