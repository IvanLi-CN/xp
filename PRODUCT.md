# Product

## Register

product

## Users

xp is for a self-hosted cluster operator who runs Xray nodes across one or more small servers and wants one reliable control plane for day-to-day management. The primary user is technical, usually the only administrator, and works in focused maintenance windows: checking node health, creating endpoints, granting user access, inspecting quota state, copying subscription material, and resolving failures without losing context.

The interface is an authenticated operational tool, not a marketing surface. It should support fast scanning, precise edits, and confident recovery when a node, endpoint, quota rule, or subscription output is wrong.

## Product Purpose

xp connects multiple `xp + xray` hosts into a lightweight Raft-backed cluster manager. It keeps cluster-wide desired state consistent, reconciles local Xray runtime state, exposes an embedded admin UI, and ships an ops CLI for installation, upgrades, container runtime, and Mihomo redaction.

Success means the operator can manage nodes, endpoints, users, subscriptions, quota policy, Reality domains, service config, and diagnostic tools from any reachable node while trusting that writes are serialized by the leader and that local runtime drift is visible and recoverable.

## Brand Personality

Calm, exacting, operational.

The product voice should sound like a precise runbook with a good interface: compact, factual, and composed under failure. It should avoid marketing flourish, fake excitement, and novelty for its own sake. Labels should name the object or action directly, especially around tokens, node IDs, endpoint IDs, public origins, quota values, and unsafe operations.

## Anti-references

- Do not make the admin UI look like a cyber or hacker toy: no neon green console pastiche, terminal rain, circuit-board decoration, or fake intrusion aesthetics.
- Do not use generic SaaS hero patterns inside the product: no hero metric panels, sales copy, decorative gradients, or repeated icon-card grids.
- Do not invent custom affordances for standard tasks: navigation, forms, dialogs, tables, command palette, and destructive confirmations should feel familiar.
- Do not over-decorate operational state. Health, role, leader, term, alerts, quota, and endpoint status must read as system facts, not visual ornaments.
- Do not hide machine values behind friendly copy. IDs, tokens, URLs, ports, and quota units need copy, monospace treatment, and predictable wrapping.

## Design Principles

1. Show cluster truth first.
   The UI should make health, leader, term, alerts, and partial failure visible before secondary decoration. If the cluster is degraded, the operator should see where trust is limited.

2. Treat density as a working tool.
   xp is used by a technical operator who benefits from dense lists, aligned fields, and quick comparison. Density must stay controlled, readable, and switchable through the existing comfortable and compact preference.

3. Keep actions close to evidence.
   Create, copy, refresh, retry, delete, and reset actions should appear beside the object they affect. Error states should retain the relevant object context and expose retry or copy-details affordances.

4. Preserve operational vocabulary.
   Use the domain words the system actually uses: node, endpoint, user, quota, subscription, Reality domain, leader, term, token, inbound, Mihomo. Avoid vague aliases that make logs and UI disagree.

5. Prefer boring reliability over expressive novelty.
   Standard product UI patterns are a feature here. The interface earns trust through consistency, state coverage, accessible contrast, and predictable keyboard and mobile behavior.

## Accessibility & Inclusion

Target WCAG 2.1 AA for the admin UI. Maintain visible focus states, non-color-only status cues, stable keyboard navigation, and readable contrast in both `xp-light` and `xp-dark`. Respect reduced-motion preferences; motion should communicate state changes only. Long machine values must wrap or scroll without covering adjacent controls, and destructive actions must remain explicit and reversible only when the backend supports it.
