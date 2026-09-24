# Implementation

## Current State

- No completed prerelease end-to-end verification is recorded.
- Treat `SPEC.md` as an active operational verification contract rather than delivered evidence.

## Required Evidence

- A qualifying prerelease release, its checksums, and the `xp-ops` dry-run selection
  must be verified.

## Verification Milestones

- Merge a minimal PR carrying `type:patch` and `channel:prerelease`.
- Confirm that the release workflow publishes a prerelease tag, assets, and checksums.
- Confirm that `xp-ops upgrade --prerelease --dry-run` selects the latest release candidate.
- Run the documented three-node Compose verification.
