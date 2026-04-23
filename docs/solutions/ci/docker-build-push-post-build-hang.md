# Docker build-push publish failures on release

## Symptoms

- GitHub Actions `release` workflow reaches `publish ghcr image` and either appears to run forever, or gets killed by the step timeout after spending a long time inside the Docker build.
- The image tags never become available in GHCR.
- The GitHub Release step never starts.
- Re-running the same workflow repeats the stall in the same place.

## Likely causes

There are two distinct failure modes behind the same `publish ghcr image` symptom:

1. `docker/build-push-action@v6` can hang during its post-build cleanup after the main build has already finished. The stall happens while exporting the build record / build summary through the newer post-build path introduced in v6.17+.
2. Even when the post-build hang is avoided, a cold multi-arch image build can still spend too long recompiling Rust inside Docker. In this repo the release workflow already builds the release binaries before the image step, so recompiling the whole project again inside `docker buildx` just burns time and can exceed a defensive timeout.

Upstream reference for the first failure mode: `docker/build-push-action` issue `#1388`.

## Fix used here

Use both mitigations together:

- set `DOCKER_BUILD_EXPORT_LEGACY=true` on the `docker/build-push-action` step so the action uses the legacy export path instead of the newer post-build build-record export path
- keep a bounded step timeout so true regressions still fail instead of hanging forever
- publish the container image from prebuilt release binaries instead of recompiling Rust inside the multi-arch Docker build

The repo already produces `release/xp-linux-x86_64`, `release/xp-linux-aarch64`, `release/xp-ops-linux-x86_64`, and `release/xp-ops-linux-aarch64` earlier in the workflow. The Dockerfile can expose a dedicated runtime target that only packages those binaries plus `xray`, `cloudflared`, and `tini`.

## Where to apply it

In `.github/workflows/release.yml`, on the `publish ghcr image` step:

- set `env.DOCKER_BUILD_EXPORT_LEGACY=true`
- set `timeout-minutes` to a bounded but realistic value
- set `target: runtime-from-prebuilt`

In `Dockerfile`:

- keep the existing source-building final image path for local `docker build`
- add a `runtime-from-prebuilt` stage that selects the correct release binaries by `TARGETARCH`

## Verification

After the fix, verify that:

- `publish ghcr image` finishes instead of hanging in post-build cleanup
- `publish ghcr image` no longer spends most of its budget recompiling Rust from source
- the `create release` step starts
- `ghcr.io/<owner>/xp:vX.Y.Z` becomes available
- `releases/tags/vX.Y.Z` stops returning 404
