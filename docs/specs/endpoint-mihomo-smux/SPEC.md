# 接入点 Mihomo SMux 策略

> 当前有效规范以本文为准。
> 实现覆盖与当前状态见 `./IMPLEMENTATION.md`，关键演进原因见 `./HISTORY.md`。

## 背景 / 问题陈述

一个客户端在同一 VLESS 或 SS2022 接入点上建立大量独立 TCP 连接，
会使节点的入站连接数显著增大。

该现象不是 Xray 入站配置能够协商解决的问题。
Mihomo 的 SMux 是客户端本地复用策略，必须随 YAML 订阅下发。

## 目标 / 非目标

### Goals

- 为现有 VLESS Reality 和 SS2022 接入点保存可编辑的 Mihomo SMux 策略。
- 所有现有 YAML 订阅输出使用该策略，既有接入点默认启用。
- 在接入点新建和详情页的高级设置中提供安全、常用的编辑项。

### Non-goals

- 不修改 Xray 入站、端口、流量路由或 TCP 回收策略。
- 不新增 sing-box、Xray 等订阅格式。
- 不向 Raw/Base64 VLESS 或 Shadowsocks URI 添加非标准参数。
- 不暴露 `max-streams`、Brutal、padding 或 statistic 的管理员调优项。

## 需求（Requirements）

### MUST

- `meta.mihomo_smux` 缺失时等价于 `enabled=true`、`max_connections=4`、
  `min_streams=4`、`only_tcp=true`。
- `max_connections` 只接受 `1..=16`，`min_streams` 只接受 `1..=64`。
- YAML 的启用配置精确包含 `enabled: true`、`protocol: smux`、`max-connections`、
  `min-streams`、`padding: false`、`statistic: false` 与 `only-tcp`；禁用时不输出
  `smux`。
- `format=clash`、`format=mihomo` 的 system provider payload 及直连/链式
  VLESS/SS2022 条目均遵守该策略。
- 管理 API 创建未传该字段时保存默认策略。
  PATCH 只接受完整对象，拒绝 `null` 与越界值。
- API 必须广告 `admin.endpoint-mihomo-smux` capability。管理 Web 在该 capability
  缺失时不显示 SMux 控件，也不得在创建或更新请求中发送 `mihomo_smux`，以便与支持
  窗口中的旧服务真实降级。
- Web 管理界面必须告知 `Mihomo >= v1.19.29` 的客户端要求。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

- `POST /api/admin/endpoints`: HTTP API, internal, Modify, Web admin。
- `PATCH /api/admin/endpoints/{endpoint_id}`: HTTP API, internal, Modify, Web admin。
- `admin.endpoint-mihomo-smux`: API capability, internal, Read, Web admin feature gate。
- YAML subscription proxy: subscription contract, external, Modify,
  Mihomo >= v1.19.29。

## 验收标准（Acceptance Criteria）

- Given 旧接入点元数据没有 `mihomo_smux`，When 渲染 YAML，Then 每个系统
  VLESS/SS2022 proxy 都带默认 `smux`。
- Given 管理员禁用一个接入点的策略，When 渲染该接入点的所有 YAML proxy，Then
  不存在 `smux` 字段，且 URI 输出不变。
- Given 管理员创建或更新 VLESS/SS2022 接入点，When 提交合法策略，Then API
  返回并持久化该完整对象。
- Given 策略数值为零、越界或 PATCH 值为 `null`，When 调用 API，Then 返回
  `400 invalid_request`。
- Given Mihomo `v1.19.29`，When 校验生成的 VLESS 与 SS2022 YAML fixture，Then 配置可被
  解析。

## 质量门槛

- Rust 单元与 HTTP 测试。
- Web lint、typecheck 与组件测试。
- Storybook 状态与受控视觉证据。
- 官方 Mihomo `v1.19.29` YAML 配置校验。

## Visual Evidence

PR: none

- source_type: storybook_canvas
  target_program: mock-only
  capture_scope: element
  requested_viewport: none
  viewport_strategy: not-applicable (no viewport-specific acceptance criterion)
  margin_policy: trim_only
  evidence_surface: page
  sensitive_exclusion: N/A
  submission_gate: pending-owner-approval
  story_id_or_title: Pages/EndpointNewPage/Mihomo Smux Defaults
  state: enabled default policy
  evidence_note: verifies the new endpoint advanced settings expose the enabled, maximum
  connections, minimum streams, and TCP-only defaults.

![New endpoint SMux advanced settings](./assets/endpoint-new-smux.png)

- source_type: storybook_canvas
  target_program: mock-only
  capture_scope: element
  requested_viewport: none
  viewport_strategy: not-applicable (no viewport-specific acceptance criterion)
  margin_policy: trim_only
  evidence_surface: page
  sensitive_exclusion: N/A
  submission_gate: pending-owner-approval
  story_id_or_title: Pages/EndpointDetailsPage/Mihomo Smux Defaults
  state: legacy endpoint fallback
  evidence_note: verifies an endpoint lacking stored policy presents the same enabled defaults in
  editable advanced settings.

![Endpoint details SMux advanced settings](./assets/endpoint-details-smux.png)

## 假设

- YAML 输出的最低兼容基线为 `Mihomo >= v1.19.29`。
- SMux 是客户端本地策略，不要求节点重启或 Xray reconcile 语义变更。
