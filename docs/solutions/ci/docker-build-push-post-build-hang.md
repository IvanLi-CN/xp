# Docker build-push post-build hang on release

## Symptoms

- GitHub Actions `release` workflow reaches `publish ghcr image` and appears to run forever.
- The image tags never become available in GHCR.
- The GitHub Release step never starts.
- Re-running the same workflow repeats the stall in the same place.

## Likely cause

`docker/build-push-action@v6` can hang during its post-build cleanup after the main build has already finished. The stall happens while exporting the build record / build summary through the newer post-build path introduced in v6.17+.

Upstream reference: `docker/build-push-action` issue `#1388`.

## Fix used here

Set `DOCKER_BUILD_EXPORT_LEGACY=true` on the `docker/build-push-action` step so the action uses the legacy export path instead of the newer post-build build-record export path.

Also add a step-level timeout to make future regressions fail fast instead of blocking release indefinitely.

## Where to apply it

In `.github/workflows/release.yml`, on the `publish ghcr image` step:

- set `env.DOCKER_BUILD_EXPORT_LEGACY=true`
- set `timeout-minutes` to a bounded value

## Verification

After the fix, verify that:

- `publish ghcr image` finishes instead of hanging in post-build cleanup
- the `create release` step starts
- `ghcr.io/<owner>/xp:vX.Y.Z` becomes available
- `releases/tags/vX.Y.Z` stops returning 404
