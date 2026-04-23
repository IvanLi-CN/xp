# Release backfill must reuse the target commit's version

## Symptoms

- A manual `workflow_dispatch` backfill for an older `main` commit computes a brand-new version instead of the version that commit originally targeted.
- The workflow then tries to push a fresh tag such as `vX.Y.Z+1` from an older commit.
- GitHub rejects the push because the old commit contains workflow files and the default Actions token does not have `workflows` permission for creating that new ref.
- The expected tag may already exist on the target commit, yet the workflow still tries to advance to a later version.

## Root cause

The original version script only looked at the repository-wide maximum semver tag, then applied the current bump level on top of that global maximum. That works for the latest successful release, but it is wrong for backfills and reruns:

- rerunning a partially failed release after the tag already exists should reuse that exact tag
- backfilling an older commit without an exact tag should derive the next version from that commit's own first-parent release history, not from newer releases that happened later

## Fix used here

Teach `.github/scripts/compute-version.sh` to resolve versions in this order:

1. If the release target commit already has the matching semver tag, reuse it directly.
2. Otherwise, walk the target commit's first-parent history backward to find the previous release baseline and derive the next stable or prerelease version floor from that target-local history.
3. Only fall back to the global maximum tag when no release target SHA is available.

This keeps `workflow_dispatch(head_sha=...)` idempotent for reruns and makes historical backfills recover the version that belongs to the requested commit.

For backfills, keep the target commit on its own release line even if later tags already exist, but still advance to the next globally free stable patch or `-rc.N` whenever the target-local version has already been consumed by a newer release.

The release workflow should also treat an existing tag as reusable only when it already points at the requested release target commit. If the same tag exists on a different commit, fail loudly instead of silently updating the wrong release.

## Verification

- Add a shell regression script that creates temporary git repos and reproduces two cases:
  - target commit already tagged, with newer tags elsewhere in history
  - target commit untagged, but newer tags exist on later commits
- Extend the regression coverage so backfills stay on the target version family, and `channel:prerelease` additionally respects the target commit's own RC floor while preserving global `-rc.N` uniqueness.
- Run that regression script in CI so future workflow changes keep the backfill behavior stable.
