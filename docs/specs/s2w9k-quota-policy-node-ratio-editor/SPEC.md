# s2w9k - Quota Policy node ratio editor + membership mapping

## Status

- Status: implemented
- Created: 2026-02-26
- Last Updated: 2026-02-26
- Flow: normal

## Goal

Deliver a node-centric quota weight editing experience in Admin Quota Policy with a visual ratio editor, while introducing a dedicated persisted node-user-endpoint membership source seeded from grants.

## Scope

- Add persisted `node_user_endpoint_memberships` in state snapshots.
- Add schema migration `v6 -> v7` to seed memberships from grants.
- Keep grant APIs and grant-group contract compatible.
- Add admin aggregate endpoint:
  - `GET /api/admin/quota-policy/nodes/:node_id/weight-rows`
- Replace quota policy weight editing UI from per-user modal to node-centric ratio editor:
  - pie chart + list rows (`User / Tier / Slider / Input / Computed weight / Lock`)
  - lock-aware redistribution with strict `total == 100%` save gate
  - partial save failure retention and retry failed rows
  - reset draft to server values
- Keep weight persistence via existing endpoint:
  - `PUT /api/admin/users/:user_id/node-weights/:node_id`

## Backend design

### Persisted state

- Schema version bumped to `7`.
- New field on `PersistedState`:
  - `node_user_endpoint_memberships: BTreeSet<NodeUserEndpointMembership>`
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
  - node selector
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
  - Storybook mock tests for new endpoint
  - schema test for new API response

## Change log

- Added membership persistence + migration to support node-centric relationship reads.
- Added admin node weight-row aggregate API.
- Added weight write audit log details.
- Reworked quota policy UI to node-centric ratio editing with visual chart and lock-aware controls.
