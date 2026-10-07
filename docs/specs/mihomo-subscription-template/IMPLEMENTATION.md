# Implementation

## Current State

- Subscription rendering, the Profile API, and the User Details editor are implemented.
- The shared full-viewport workspace is implemented for User Details and Demo User Details.
- Profile API and subscription contracts remain unchanged.

## Recorded Delivery State

- Status: complete for the Mihomo profile workspace scope
- Created: 2026-03-04
- Last: 2026-10-07

## Implementation Milestones

- [x] M1: Persist the user Mihomo Profile and expose the Profile API.
- [x] M2: Keep `mixin_yaml` as the public Profile field.
- [x] M3: Keep provider-only subscription rendering and dynamic groups.
- [x] M4: Add the shared User Details and Demo workspace.
- [x] M5: Preserve drafts, editor state, permissions, and save protection.
- [x] M6: Add Storybook states and real CodeMirror browser coverage.

## Workspace Coverage

- `REQ-MIHOMO-WORKSPACE-001`: Expand entry, full viewport, same URL, and user context.
- `REQ-MIHOMO-WORKSPACE-002`: Three fixed documents share mounted CodeMirror instances.
- `REQ-MIHOMO-WORKSPACE-003`: Draft Hook protects dirty state and navigation guards.
- `REQ-MIHOMO-WORKSPACE-004`: One save submits all fields and keeps retryable errors.
- `REQ-MIHOMO-WORKSPACE-005`: Desktop tree, mobile drawer, dynamic viewport, and scrolling.
- `REQ-MIHOMO-WORKSPACE-006`: Theme, labels, focus restoration, keyboard, and read-only mode.

## Validation Evidence

- Hook tests cover dirty refresh, duplicate saves, session changes, and late responses.
- Save/query generations reject stale PUT responses and preserve the current draft and cache.
- Production/Demo links, list rows, app navigation, logout, and delete use the dirty guard.
- The guard detects pending Mihomo saves, blocks discard, and reuses the request.
- User Details tests cover the Profile API payload and normalization behavior.
- Playwright covers Demo desktop/mobile and real CodeMirror state for all documents.
- Playwright covers User Details expansion, dirty leave/delete, and three-field saves.
- Storybook covers inline, expanded, saving, read-only, save-error, and mobile drawer states.
- `cd web && bun run lint` passes.
- `cd web && bun run typecheck` passes.
- `python3 scripts/check-style-budget.py` passes.
- `cd web && bun run build` passes.
- Storybook Mihomo workspace interactions pass.
- E2E passes at 320/360/393/768/1440px with continuity, Viewer, Files focus, and dirty navigation.
- The full frontend suite passes with 107 test files and 516 tests.
- The Impeccable detector reports no findings for the polished workspace and navigation surfaces.

## Scope Notes

- `YamlCodeEditor` keeps fixed-height behavior by default and fills its container in the workspace.
- ToolsPage, preview, backend routes, Fullscreen API, autosave, and refresh are out of scope.

## Visual Evidence

source_type=storybook_canvas_and_demo_e2e
target_program=mock-only
capture_scope=browser-viewport
sensitive_exclusion=N/A; controlled fixtures only
requested_viewports=1440x900,393x852
submission_gate=approved-by-owner

![Mihomo workspace desktop](./assets/mihomo-workspace-dark-desktop.png)
![Mihomo workspace mobile editor](./assets/mihomo-workspace-dark-mobile.png)
![Mihomo workspace mobile Files drawer](./assets/mihomo-workspace-dark-files.png)
