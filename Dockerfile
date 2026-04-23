# syntax=docker/dockerfile:1.7

ARG XRAY_DOCKER_TAG=25.12.8
ARG BUN_VERSION=1.3.5
ARG XP_BUILD_VERSION=dev

FROM oven/bun:${BUN_VERSION} AS web-builder
WORKDIR /app/web
COPY web/package.json web/bun.lock ./
RUN bun install --frozen-lockfile
COPY web/ ./
RUN bun run build

FROM rust:1.91.0-bookworm AS builder
ARG XP_BUILD_VERSION=dev
ENV XP_BUILD_VERSION=${XP_BUILD_VERSION}
WORKDIR /app

RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates build-essential pkg-config \
  && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock build.rs rust-toolchain.toml ./
COPY proto ./proto
COPY src ./src
COPY tests ./tests
COPY --from=web-builder /app/web/dist ./web/dist

RUN --mount=type=cache,target=/usr/local/cargo/registry \
  --mount=type=cache,target=/usr/local/cargo/git \
  --mount=type=cache,target=/app/target \
  mkdir -p /out \
  && cargo build --release --locked --bin xp --bin xp-ops \
  && cp /app/target/release/xp /out/xp \
  && cp /app/target/release/xp-ops /out/xp-ops

FROM ghcr.io/xtls/xray-core:${XRAY_DOCKER_TAG} AS xray

FROM debian:bookworm-slim AS runtime-base
ARG XP_BUILD_VERSION=dev
ENV XP_BUILD_VERSION=${XP_BUILD_VERSION}
LABEL org.opencontainers.image.source="https://github.com/IvanLi-CN/xp"
LABEL org.opencontainers.image.description="xp single-image cluster node runtime"

RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl tini \
  && curl -fsSL https://pkg.cloudflare.com/cloudflare-main.gpg -o /usr/share/keyrings/cloudflare-main.gpg \
  && printf '%s\n' 'deb [signed-by=/usr/share/keyrings/cloudflare-main.gpg] https://pkg.cloudflare.com/cloudflared any main' > /etc/apt/sources.list.d/cloudflared.list \
  && apt-get update \
  && apt-get install -y --no-install-recommends cloudflared \
  && rm -rf /var/lib/apt/lists/*

COPY --from=xray /usr/local/bin/xray /usr/local/bin/xray

RUN mkdir -p /var/lib/xp/data /etc/cloudflared /etc/xp-ops/cloudflare_tunnel /etc/xray

VOLUME ["/var/lib/xp/data", "/etc/cloudflared", "/etc/xp-ops/cloudflare_tunnel"]
EXPOSE 62416
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=15s --timeout=5s --start-period=20s --retries=6 CMD curl -fsS http://127.0.0.1:62416/api/health >/dev/null || exit 1
ENTRYPOINT ["tini", "--", "/usr/local/bin/xp-ops", "container", "run"]

FROM runtime-base AS runtime-from-source
COPY --from=builder /out/xp /usr/local/bin/xp
COPY --from=builder /out/xp-ops /usr/local/bin/xp-ops

FROM runtime-base AS runtime-from-prebuilt
ARG TARGETARCH
RUN --mount=type=bind,source=release,target=/tmp/release,ro \
  set -eu; \
  case "${TARGETARCH}" in \
    amd64) suffix='x86_64' ;; \
    arm64) suffix='aarch64' ;; \
    *) echo "unsupported TARGETARCH=${TARGETARCH}" >&2; exit 1 ;; \
  esac; \
  cp "/tmp/release/xp-linux-${suffix}" /usr/local/bin/xp; \
  cp "/tmp/release/xp-ops-linux-${suffix}" /usr/local/bin/xp-ops; \
  chmod 0755 /usr/local/bin/xp /usr/local/bin/xp-ops

FROM runtime-from-source AS runtime
