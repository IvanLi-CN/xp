# Geo DB 自更新与设置页（DB-IP Lite / 集群统一配置）

## 背景

现有入站 IP 使用详情依赖节点本地 MMDB 文件，但数据库分发与更新仍靠运营方手工放置 GeoLite2 文件。结果是：

- 节点间数据库版本不一致，地区/运营商结果漂移。
- 缺库时只能展示 warning，不能由 xp 自行恢复。
- Settings 里没有统一的 Geo DB 状态、调度与手动更新入口。

本规格将 Geo DB 扩展为 xp 自管能力：集群统一设置由 Raft 持久化，各节点本地执行 DB-IP Lite City + ASN MMDB 下载、热更新与缓存回填，同时兼容显式外部路径覆盖。

## 目标

- 由 xp 托管 `DB-IP Lite City + ASN MMDB` 的本地下载、替换、热重载与状态展示。
- 提供集群统一的 Settings 页面与 HTTP API，支持自动更新开关、更新频率、手动触发全节点更新。
- 让 Node/User IP usage 在不重启 xp 的情况下立即切换到新 Geo DB，并对现有缓存 IP 回填地区/运营商。
- 保留 `XP_IP_USAGE_CITY_DB_PATH` / `XP_IP_USAGE_ASN_DB_PATH` 兼容口径；显式配置时进入 `external_override` 模式。

## 非目标

- 不引入在线 IP API。
- 不做 Geo provider 多选、凭据管理、版本回滚、差分下载。
- 不把可写控制塞进现有只读的 `Service config` 页面。

## 范围

### In scope

- Raft 状态机新增 `geo_db_update_settings`，schema 从 `v10` 升到 `v11`。
- 本地托管路径固定为 `XP_DATA_DIR/geoip/dbip-city-lite.mmdb`、`XP_DATA_DIR/geoip/dbip-asn-lite.mmdb`。
- 本地 runtime 状态文件固定为 `XP_DATA_DIR/geoip_update_runtime.json`。
- 新增公开 API：
  - `GET /api/admin/ip-geo-db`
  - `PATCH /api/admin/ip-geo-db`
  - `POST /api/admin/ip-geo-db/update`
- 新增 internal local API：
  - `GET /api/admin/_internal/ip-geo-db/local`
  - `POST /api/admin/_internal/ip-geo-db/update`
- Node/User IP usage 响应增加 `geo_source`，取值固定为 `managed_dbip_lite | external_override | missing`。
- 新增 Settings 路由 `/ip-geo-db`，导航名称固定为 `IP geolocation`。

### Out of scope

- 外部在线查询与自动下载 GeoLite2 / MaxMind。
- 按节点单独配置自动更新策略。
- 历史数据库版本管理。

## 数据与状态模型

### 集群配置（Raft）

`geo_db_update_settings` 固定结构：

- `provider`: `dbip_lite`
- `auto_update_enabled`: `bool`
- `update_interval_days`: `1..=30`

迁移默认值：

- `provider=dbip_lite`
- `auto_update_enabled=false`
- `update_interval_days=1`

### 节点本地运行状态

运行状态只保存在本地 `geoip_update_runtime.json`，不进 Raft。节点状态至少包含：

- `mode`: `managed | external_override | missing`
- `running`
- `city_db_path`
- `asn_db_path`
- `last_started_at`
- `last_success_at`
- `next_scheduled_at`
- `last_error`

### IP usage Geo source

- `managed_dbip_lite`: 当前节点使用 xp 托管的 DB-IP Lite MMDB。
- `external_override`: 当前节点由显式 env 路径托管；xp 跳过下载与替换。
- `missing`: 当前节点没有可用的 City + ASN 库。

## 功能规格

### 1. 自动更新 worker

- leader 只负责保存统一设置；真正的下载、替换与回填在各节点本地执行。
- 本地 worker 启动后立即评估当前状态：
  - 若 `external_override`，不下载，状态标记为 `external_override`。
  - 若 `auto_update_enabled=true` 且本地库缺失，应立即触发首次托管下载。
  - 若已有成功记录，则按 `last_success_at + update_interval_days` 计算下次调度。
- 手动触发不修改全局设置，即使 `auto_update_enabled=false` 也允许执行。
- 同一节点同一时刻只允许一个更新任务运行；重复触发返回 `already_running`。

### 2. 下载与替换

- provider 固定为官方 `DB-IP Lite City + ASN MMDB`。
- 只有当 City + ASN 两个库都成功下载、解压、校验后，才允许替换现网托管文件。
- 失败时保留旧库并写入 `last_error`，不得影响 quota/IP usage 主流程。
- 更新成功后必须热重载 Geo resolver，后续 minute sample 立即使用新库。

### 3. Geo cache 回填

- 下载成功后无需重启 xp。
- worker 必须对 `inbound_ip_usage.json` 中已缓存的全部 IP 重新解析 `country / region / city / operator` 并回写。
- 回填失败不得破坏历史 minute bitmap，只影响 geo/operator 字段更新。

### 4. API 契约

#### `GET /api/admin/ip-geo-db`

返回：

- 统一设置
- `partial`
- `unreachable_nodes`
- 每节点本地状态列表

#### `PATCH /api/admin/ip-geo-db`

- 仅允许修改 `auto_update_enabled` 与 `update_interval_days`
- `update_interval_days` 超出 `1..=30` 返回 `invalid_request`
- 返回更新后的统一设置

#### `POST /api/admin/ip-geo-db/update`

- fan-out 到全部节点，启动后台任务后立即返回
- 每节点结果固定为 `accepted | already_running | skipped | error`
- 远端不可达时 `partial=true` 并列入 `unreachable_nodes`

### 5. UI 语义

#### Settings / IP geolocation

页面必须包含：

- provider 说明（`DB-IP Lite City + ASN MMDB`）
- 自动更新开关
- 更新频率（1-30 天）
- 保存按钮
- 手动触发全节点更新按钮
- 每节点状态卡/表

交互规则：

- 当任一节点 `running=true` 时，前端每 2 秒轮询 `GET /api/admin/ip-geo-db`
- 手动更新按钮在触发后进入 loading，直到全部节点退出 running
- `external_override` 节点明确标记为 externally managed

#### Node / User IP usage

- `managed_dbip_lite`: 展示 `DB-IP Lite` attribution
- `external_override`: 展示“该节点 Geo DB 由外部文件托管”提示
- `missing`: 展示缺库 warning，列表中 `region/operator` 为 `Unknown`

## 验收标准

- Given 现有集群升级到新版本，When 节点加载 `state.json`，Then 自动得到 `geo_db_update_settings={provider:dbip_lite, auto_update_enabled:false, update_interval_days:1}`。
- Given 节点未设置外部 env 路径且本地 DB 缺失，When 开启自动更新，Then 节点会立即拉起第一次 DB-IP Lite 下载。
- Given 任一数据库下载或校验失败，When 更新任务结束，Then 旧库保持可用，`last_success_at` 不回退，`last_error` 更新。
- Given 更新成功，When 不重启 xp 继续采集与查询，Then 新 geo/operator 解析立即生效，并且已有 `inbound_ip_usage.json` 记录被回填。
- Given 管理员调用 `POST /api/admin/ip-geo-db/update`，When 某节点正在运行更新任务，Then 该节点结果为 `already_running`。
- Given 某节点显式配置 `XP_IP_USAGE_CITY_DB_PATH` / `XP_IP_USAGE_ASN_DB_PATH`，When 查询设置页或手动更新，Then 该节点显示 `external_override` 并跳过托管更新。
- Given 节点/用户 IP usage 查询返回 `geo_source=managed_dbip_lite`，When 页面渲染，Then 展示 `DB-IP Lite` attribution；Given `external_override`，Then 展示 externally managed 提示；Given `missing`，Then 展示缺库 warning。

## 质量门槛

### Backend

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`

### Web

- `cd web && bun run lint`
- `cd web && bun run typecheck`
- `cd web && bun run test`
- `cd web && bun run build-storybook`
- `cd web && bun run test-storybook`

## 文档同步

- `docs/desgin/api.md`
- `docs/ops/README.md`
- `docs/ops/env/xp.env.example`
- 相关 IP usage / ops 文档中所有“GeoLite2 手工放置”描述

## 实现里程碑

- [ ] M1: schema v11、Raft 设置命令与 admin PATCH/GET 契约落地
- [ ] M2: 本地 Geo DB updater、热重载、回填与 internal fan-out 落地
- [ ] M3: Settings 页面、IP usage attribution、Storybook 与测试落地
- [ ] M4: 全量验证、提交、PR、checks 与 review-loop 收敛

## 参考

- `src/state.rs`
- `src/inbound_ip_usage.rs`
- `src/http/mod.rs`
- `src/main.rs`
- `web/src/components/IpUsageView.tsx`
- `web/src/views/ServiceConfigPage.tsx`
