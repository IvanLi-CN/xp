# s2w9k - Quota Policy node ratio editor + membership mapping

## Status

- Status: in_progress
- Created: 2026-02-26
- Last Updated: 2026-02-26
- Flow: normal

## Goal

Deliver a quota policy editing experience that supports:

- a global default allocation rule,
- node-specific allocation overrides,
- inherit-global behavior enabled by default (with a switch),
- and a node-centric visual ratio editor.

The membership source migration from grants remains part of the same spec.

## Scope

- Add persisted `node_user_endpoint_memberships` in state snapshots.
- Add schema migration `v6 -> v7` to seed memberships from grants.
- Keep grant APIs and grant-group contract compatible.
- Add admin aggregate endpoint:
  - `GET /api/admin/quota-policy/nodes/:node_id/weight-rows`
- Add global default allocation rule editor (admin only).
- Add node-level `inherit_global` policy switch:
  - when enabled, node uses global default allocation
  - when disabled, node uses node-specific weights
- Replace quota policy weight editing UI from per-user modal to node-centric ratio editor:
  - pie chart + list rows (`User / Tier / Slider / Input / Computed weight / Lock`)
  - lock-aware redistribution with strict `total == 100%` save gate
  - partial save failure retention and retry failed rows
  - reset draft to server values
- Keep weight persistence via existing endpoint:
  - `PUT /api/admin/users/:user_id/node-weights/:node_id`

## Requirement updates (global + node rules)

### Rule model

- Global default rule:
  - defines baseline per-user weight ratios shared by nodes.
- Node rule:
  - can inherit global default (`inherit_global = true`, default),
  - or use node-specific ratios (`inherit_global = false`).

### Effective weight resolution

- Runtime effective weight for `(user, node)`:
  - if node has `inherit_global = false` and explicit node rule value exists, use node value;
  - otherwise use global default value.
- Existing quota distribution formula remains unchanged.

### Editing behavior

- Global editor:
  - edits global default ratio table and persists integer weights.
- Node editor:
  - shows `inherit_global` switch.
  - when inherit is on:
    - show effective ratio preview from global rule (read-only for node-local values).
  - when inherit is off:
    - enable node-local ratio editing.
    - first toggle-off initializes local draft from current global effective ratios.

### Defaults and compatibility

- New node policy default: `inherit_global = true`.
- Existing nodes without policy are treated as inherit-global during migration/backfill.
- Existing explicit node weights remain valid and are used when node policy disables inheritance.

## Backend design

### Persisted state

- Schema version bumped to `7`.
- New field on `PersistedState`:
  - `node_user_endpoint_memberships: BTreeSet<NodeUserEndpointMembership>`
- Add persisted global default weight set (admin-managed).
- Add persisted node policy metadata:
  - per-node `inherit_global` flag.
- New value object:
  - `NodeUserEndpointMembership { user_id, node_id, endpoint_id }`

### Migration and consistency

- `migrate_v6_to_v7` seeds memberships from current grants + endpoints.
- Startup normalization sanitizes malformed membership entries and re-syncs memberships from grants.
- Commands that can affect relationships now re-sync memberships after apply:
  - node/endpoint/grant/group/user mutations that alter relationships.

### Admin API

- New route:
  - `GET /api/admin/quota-policy/nodes/:node_id/weight-rows`
- Add global rule endpoints (admin only):
  - `GET /api/admin/quota-policy/global-weight-rows`
  - `PUT /api/admin/quota-policy/global-weight-rows/:user_id`
- Add node policy endpoints (admin only):
  - `GET /api/admin/quota-policy/nodes/:node_id/policy`
  - `PUT /api/admin/quota-policy/nodes/:node_id/policy`
- Row payload:
  - `user_id`
  - `display_name`
  - `priority_tier`
  - `endpoint_ids[]`
  - `stored_weight?`
  - `editor_weight`
  - `source: explicit | implicit_zero`
- Sort order:
  - `editor_weight desc`, then `user_id asc`.

### Conflict policy

- Keep last-write-wins (no version token in this scope).

### Weight write audit

- `PUT /api/admin/users/:user_id/node-weights/:node_id` now emits audit log fields:
  - actor source (`admin_token` or `internal_signature`)
  - `user_id`, `node_id`
  - `old_weight`, `new_weight`, `changed`
  - timestamp

## Frontend design

### New API client

- `web/src/api/adminQuotaPolicyNodeWeightRows.ts`
- Zod schemas for new weight-row response.

### Ratio editor model

- `web/src/utils/quotaPolicyWeights.ts` provides:
  - lock-aware redistribution
  - `weights -> basis points` normalization
  - `basis points -> integer weights` (largest remainder)
  - percent parsing and formatting

### Quota policy page

- `web/src/views/QuotaPolicyPage.tsx` now uses node-centric ratio editor.
- Old per-user weight modal entry removed.
- Features:
  - global default editor entry
  - node selector
  - node-level inherit-global switch
  - top pie chart with hover linkage
  - bottom editable rows (slider + percent input + lock)
  - strict save gating on `100%`
  - partial failure retention and `Retry failed rows`
  - reset to server values
  - last save status/time

## Validation

Implemented automated coverage additions:

- Rust:
  - migration membership seeding
  - relationship sync in grant mutation path
  - admin weight-row endpoint behavior
  - auth guard for new endpoint
- Web:
  - ratio utility tests (redistribution, constraints, conversion)
  - quota policy page behavior tests (input linkage, partial failure retry)
  - inherit-global switch behavior tests (read-only inherited view vs editable local override)
  - Storybook mock tests for new endpoint
  - schema test for new API response

## Acceptance criteria addendum

- Admin can edit a global default allocation rule.
- Every node defaults to inherit-global and can toggle to node-local override.
- In inherited mode, node view reflects global ratios and cannot accidentally mutate node-local values.
- After switching to node-local mode, edits persist only to node-local weights and do not mutate global defaults.

## Change log

- Added membership persistence + migration to support node-centric relationship reads.
- Added admin node weight-row aggregate API.
- Added weight write audit log details.
- Reworked quota policy UI to node-centric ratio editing with visual chart and lock-aware controls.
- Added requirement delta: global default allocation + node inherit/override policy switch.
