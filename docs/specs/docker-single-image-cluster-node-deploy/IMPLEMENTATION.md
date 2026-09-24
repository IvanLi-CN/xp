# Implementation

## Current State

- The single-image Docker cluster-node deployment contract in `SPEC.md` is implemented.
- Container behavior remains subject to the supported-environment constraints in the
  operations docs.

## Recorded Delivery State

- Status: 已完成
- Created: 2026-04-23
- Last: 2026-08-04

## 实现里程碑（Milestones / Delivery checklist）

- M1: 建立 spec，冻结单镜像 / Tunnel / GHCR / 卷契约。
- M2: 实现 `xp-ops container run` 与 Cloudflare container runtime 复用。
- M3: 交付正式 Dockerfile 与 Compose 示例。
- M4: 扩展 CI / Release 到 Docker smoke + GHCR 多架构发布。
- M5: 同步 README / ops 文档。
