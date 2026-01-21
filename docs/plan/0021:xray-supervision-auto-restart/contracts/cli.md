# CLI / Config Contracts（#0021）

本文件用于冻结“`xp` 监测并恢复 `xray`”相关的最小配置形状，确保行为可配置、可测试且升级兼容。

## Scope

- 仅影响 `xp` 运行时（`xp run`）。
- 本计划要求该探活能力**默认启用**（无额外开关），但它只负责：
  - 周期性探活 `xray` gRPC；
  - 输出 health 状态（见 `contracts/http-apis.md`）；
  - 在 `down -> up` 边沿触发一次 full reconcile。
- `xp` 不负责启动/重启 `xray`（init system 托管）。

## Config keys

> 说明：下列配置项以 env var 为主，同时允许提供等价的 CLI flag（实现阶段二选一并固化，但语义必须一致）。

### Health check

- `XP_XRAY_HEALTH_INTERVAL_SECS` (int)
  - Default: `2`
  - Range: `1..=30`
  - Meaning: gRPC 探活间隔（越小越“及时”，但也更易放大故障与日志噪声）。
- `XP_XRAY_HEALTH_FAILS_BEFORE_DOWN` (int)
  - Default: `3`
  - Range: `1..=10`
  - Meaning: 连续失败次数达到该阈值后，将 `xray.status` 置为 `down` 并记录 `down_since`。

SLO notes (normative):

- 本计划的“`10s` 内判定 down/恢复后触发 reconcile”验收，默认以推荐配置为准：
  - `XP_XRAY_HEALTH_INTERVAL_SECS=2`
  - `XP_XRAY_HEALTH_FAILS_BEFORE_DOWN=3`
- 若运维侧调大这两个值，则 down 判定与 up 检测时间会相应变慢；经验上可近似为：
  - down 判定：`interval * fails`（秒级）
  - up 检测：最多 `interval`（秒级）

## Recovery & ownership boundary (normative)

- `xp` MUST NOT 启动/拉起/重启 `xray` 进程。
- `xray` MUST 由 init system 托管并负责自动重启（例如 systemd 的 `Restart=on-failure`，或 OpenRC 的 supervise-daemon 等等）。

## Compatibility rules

- 新增配置项必须为可选并有默认值，确保升级后不需要立即改配置。
- 对 `XP_XRAY_API_ADDR` 的含义保持不变：它仍是 `xp` 连接 `xray` gRPC 的目标地址。
